const zksync = require('zksync-ethers');
const ethers = require('ethers');

// ---- constants ----

// Fixed system contract on zkSync L2
const INTEROP_CENTER_ADDRESS = '0x0000000000000000000000000000000000010010';
const INTEROP_HANDLER_ADDRESS = '0x000000000000000000000000000000000001000d';

// Address of the contract with interopRoots mapping
const L2_INTEROP_ROOT_STORAGE = '0x0000000000000000000000000000000000010008';

// Bridge system contracts
const L2_ASSET_ROUTER_ADDRESS = '0x0000000000000000000000000000000000010003';
const L2_NATIVE_TOKEN_VAULT_ADDRESS = '0x0000000000000000000000000000000000010004';

// Encoding version for bridge data
const NEW_ENCODING_VERSION = '0x01';


const InteropRootStorageAbi = [
  'function interopRoots(uint256 chainId, uint256 batchNumber) view returns (bytes32)'
];
// Wait until the interop root for (chainId, batchNumber) becomes available and matches expected root
async function waitUntilRootBecomesAvailable(wallet, chainId, batchNumber, expectedRoot) {
  const contract = new zksync.Contract(L2_INTEROP_ROOT_STORAGE, InteropRootStorageAbi, wallet.provider);
  const POLL_INTERVAL = 1000; // ms
  const DEFAULT_TIMEOUT = 300_000; // ms (5 minutes)
  let retries = Math.floor(DEFAULT_TIMEOUT / POLL_INTERVAL);
  while (retries > 0) {
    let root;
    try {
      root = await contract.interopRoots(chainId, batchNumber);
    } catch (e) {
      root = null;
    }
    if (root && root !== ethers.ZeroAddress && root !== '0x0000000000000000000000000000000000000000000000000000000000000000') {
      if (root.toLowerCase() === expectedRoot.toLowerCase()) {
        return;
      } else {
        throw new Error(`Interop root mismatch: expected ${expectedRoot}, got ${root}`);
      }
    }
    retries -= 1;
    await new Promise((res) => setTimeout(res, POLL_INTERVAL));
  }
  throw new Error('Interop root did not become available in time');
}

const IInteropCenterAbi = [
  'function sendBundle(bytes calldata _destinationChainId, tuple(bytes to, bytes data, bytes[] callAttributes)[] calldata _callStarters, bytes[] calldata _bundleAttributes) external payable returns (bytes32)'
];

const IInteropHandlerAbi = [
  'function executeBundle(bytes memory _bundle, tuple(uint256 chainId, uint256 l1BatchNumber, uint256 l2MessageIndex, tuple(uint16 txNumberInBatch, address sender, bytes data) message, bytes32[] proof) memory _proof) external'
];

const ERC20Abi = [
  'constructor(uint256 _initialSupply)',
  'function balanceOf(address account) view returns (uint256)',
  'function transfer(address to, uint256 value) returns (bool)',
  'function approve(address spender, uint256 value) returns (bool)',
  'function name() view returns (string)',
  'function symbol() view returns (string)',
  'function decimals() view returns (uint8)'
];

const IL2NativeTokenVaultAbi = [
  'function assetId(address _tokenAddress) view returns (bytes32)'
];

// ---- helpers ----

// Helper to convert chainid to minimal bytes representation (similar to Solidity _toChainReference)
function toChainReference(chainid) {
  if (chainid === 0n) {
    return new Uint8Array([0]);
  }

  const hex = chainid.toString(16);
  const paddedHex = hex.length % 2 === 0 ? hex : '0' + hex;
  return ethers.getBytes('0x' + paddedHex);
}

// Format ERC-7930 interoperable address for EVM chain with address
function formatEvmV1WithAddress(chainid, addr) {
  const chainReference = toChainReference(BigInt(chainid));
  return ethers.concat([
    '0x00010000',  // version (0x0001) + chainType (0x0000 for EVM)
    ethers.toBeHex(chainReference.length, 1),
    chainReference,
    ethers.toBeHex(20, 1),  // address length
    addr
  ]);
}

// Format ERC-7930 interoperable address for EVM chain without address
function formatEvmV1(chainid) {
  const chainReference = toChainReference(BigInt(chainid));
  return ethers.concat([
    '0x00010000',  // version (0x0001) + chainType (0x0000 for EVM)
    ethers.toBeHex(chainReference.length, 1),
    chainReference,
    ethers.toBeHex(0, 1)  // address length = 0
  ]);
}

// Format ERC-7930 interoperable address with just address (no chain reference)
// This is used for the 'to' field in InteropCallStarter
function formatEvmV1AddressOnly(addr) {
  return ethers.concat([
    '0x000100000014',  // version (0x0001) + chainType (0x0000) + chainRefLength (0x00) + addrLength (0x14 = 20)
    addr
  ]);
}

// Helper to compute asset ID (similar to DataEncoding.encodeNTVAssetId)
function computeAssetId(chainId, tokenAddress) {
  return ethers.keccak256(
    ethers.AbiCoder.defaultAbiCoder().encode(
      ['uint256', 'address', 'address'],
      [chainId, L2_NATIVE_TOKEN_VAULT_ADDRESS, tokenAddress]
    )
  );
}

// Helper to encode bridge burn data
function encodeBridgeBurnData(amount, receiver, maybeTokenAddress) {
  return ethers.AbiCoder.defaultAbiCoder().encode(
    ['uint256', 'address', 'address'],
    [amount, receiver, maybeTokenAddress]
  );
}

// Helper to encode asset router bridgehub deposit data
function encodeAssetRouterBridgehubDepositData(assetId, transferData) {
  return ethers.concat([
    NEW_ENCODING_VERSION,
    ethers.AbiCoder.defaultAbiCoder().encode(
      ['bytes32', 'bytes'],
      [assetId, transferData]
    )
  ]);
}

// Helper to build "second bridge" calldata
function buildSecondBridgeCalldata(assetId, amount, receiver, maybeTokenAddress) {
  const inner = encodeBridgeBurnData(amount, receiver, maybeTokenAddress);
  return encodeAssetRouterBridgehubDepositData(assetId, inner);
}

async function waitUntilBlockFinalized(wallet, blockNumber) {
    console.log('Waiting for block to be finalized...', blockNumber);
    // Similar to Rust: poll for finalized block, with retries and interval
    const POLL_INTERVAL = 100; // ms
    const DEFAULT_TIMEOUT = 300_000; // ms (5 minutes)
    let retries = Math.floor(DEFAULT_TIMEOUT / POLL_INTERVAL);
    while (retries > 0) {
      // 'finalized' block is mapped to the latest executed block
      let executedBlock;
      try {
        const block = await wallet.provider.getBlock('finalized');
        executedBlock = block ? block.number : 0;
      } catch (e) {
        executedBlock = 0;
      }
      if (executedBlock >= blockNumber) {
        // Block is finalized
        return;
      } else {
        // Optionally log debug info
        // console.debug(`Block not finalized yet: executedBlock=${executedBlock}, expected=${blockNumber}`);
        retries -= 1;
        await new Promise((res) => setTimeout(res, POLL_INTERVAL));
      }
    }
    throw new Error('Block was not finalized in time');
}

async function waitForL2ToL1LogProof(wallet, blockNumber, txHash) {
    // First, we wait for block to be finalized.
    await waitUntilBlockFinalized(wallet, blockNumber);

    // Second, we wait for the log proof.
    while ((await wallet.provider.getLogProof(txHash, 0)) == null) {
        // console.log('Waiting for log proof...');
        await zksync.utils.sleep(wallet.provider.pollingInterval);
    }
}

function requireEnv(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`Missing env var: ${name}`);
  }
  return value;
}

// ---- main ----

async function main() {
  const PRIVATE_KEY = requireEnv('PRIVATE_KEY');
  const L2_RPC_URL = requireEnv('L2_RPC_URL');
  const L2_RPC_URL_SECOND = requireEnv('L2_RPC_URL_SECOND');

  const providerA = new zksync.Provider(L2_RPC_URL);
  const providerB = new zksync.Provider(L2_RPC_URL_SECOND);

  const walletA = new zksync.Wallet(PRIVATE_KEY, providerA);
  const walletB = new zksync.Wallet(PRIVATE_KEY, providerB);

  console.log('Wallet address:', walletA.address);

  // ---- Deploy ERC20 token on chain A ----

  console.log('\n=== DEPLOYING ERC20 TOKEN ===');

  // Compile contract bytecode (from solc SimpleERC20.sol --bin)
  const tokenBytecode = '0x60806040526040518060400160405280600a81526020017f5465737420546f6b656e000000000000000000000000000000000000000000008152505f9081620000499190620003f9565b506040518060400160405280600481526020017f544553540000000000000000000000000000000000000000000000000000000081525060019081620000909190620003f9565b50601260025f6101000a81548160ff021916908360ff160217905550348015620000b8575f80fd5b50604051620012f5380380620012f58339818101604052810190620000de919062000510565b806003819055508060045f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f20819055503373ffffffffffffffffffffffffffffffffffffffff165f73ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8360405162000186919062000551565b60405180910390a3506200056c565b5f81519050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52604160045260245ffd5b7f4e487b71000000000000000000000000000000000000000000000000000000005f52602260045260245ffd5b5f60028204905060018216806200021157607f821691505b602082108103620002275762000226620001cc565b5b50919050565b5f819050815f5260205f209050919050565b5f6020601f8301049050919050565b5f82821b905092915050565b5f600883026200028b7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff826200024e565b6200029786836200024e565b95508019841693508086168417925050509392505050565b5f819050919050565b5f819050919050565b5f620002e1620002db620002d584620002af565b620002b8565b620002af565b9050919050565b5f819050919050565b620002fc83620002c1565b620003146200030b82620002e8565b8484546200025a565b825550505050565b5f90565b6200032a6200031c565b62000337818484620002f1565b505050565b5b818110156200035e57620003525f8262000320565b6001810190506200033d565b5050565b601f821115620003ad5762000377816200022d565b62000382846200023f565b8101602085101562000392578190505b620003aa620003a1856200023f565b8301826200033c565b50505b505050565b5f82821c905092915050565b5f620003cf5f1984600802620003b2565b1980831691505092915050565b5f620003e98383620003be565b9150826002028217905092915050565b620004048262000195565b67ffffffffffffffff81111562000420576200041f6200019f565b5b6200042c8254620001f9565b6200043982828562000362565b5f60209050601f8311600181146200046f575f84156200045a578287015190505b620004668582620003dc565b865550620004d5565b601f1984166200047f866200022d565b5f5b82811015620004a85784890151825560018201915060208501945060208101905062000481565b86831015620004c85784890151620004c4601f891682620003be565b8355505b6001600288020188555050505b505050505050565b5f80fd5b620004ec81620002af565b8114620004f7575f80fd5b50565b5f815190506200050a81620004e1565b92915050565b5f60208284031215620005285762000527620004dd565b5b5f6200053784828501620004fa565b91505092915050565b6200054b81620002af565b82525050565b5f602082019050620005665f83018462000540565b92915050565b610d7b806200057a5f395ff3fe608060405234801561000f575f80fd5b5060043610610091575f3560e01c8063313ce56711610064578063313ce5671461013157806370a082311461014f57806395d89b411461017f578063a9059cbb1461019d578063dd62ed3e146101cd57610091565b806306fdde0314610095578063095ea7b3146100b357806318160ddd146100e357806323b872dd14610101575b5f80fd5b61009d6101fd565b6040516100aa919061094e565b60405180910390f35b6100cd60048036038101906100c891906109ff565b610288565b6040516100da9190610a57565b60405180910390f35b6100eb610375565b6040516100f89190610a7f565b60405180910390f35b61011b60048036038101906101169190610a98565b61037b565b6040516101289190610a57565b60405180910390f35b61013961065b565b6040516101469190610b03565b60405180910390f35b61016960048036038101906101649190610b1c565b61066d565b6040516101769190610a7f565b60405180910390f35b610187610682565b604051610194919061094e565b60405180910390f35b6101b760048036038101906101b291906109ff565b61070e565b6040516101c49190610a57565b60405180910390f35b6101e760048036038101906101e29190610b47565b6108a4565b6040516101f49190610a7f565b60405180910390f35b5f805461020990610bb2565b80601f016020809104026020016040519081016040528092919081815260200182805461023590610bb2565b80156102805780601f1061025757610100808354040283529160200191610280565b820191905f5260205f20905b81548152906001019060200180831161026357829003601f168201915b505050505081565b5f8160055f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f20819055508273ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff167f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925846040516103639190610a7f565b60405180910390a36001905092915050565b60035481565b5f8160045f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205410156103fc576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016103f390610c2c565b60405180910390fd5b8160055f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205410156104b7576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016104ae90610c94565b60405180910390fd5b8160045f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105039190610cdf565b925050819055508160045f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105569190610d12565b925050819055508160055f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105e49190610cdf565b925050819055508273ffffffffffffffffffffffffffffffffffffffff168473ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef846040516106489190610a7f565b60405180910390a3600190509392505050565b60025f9054906101000a900460ff1681565b6004602052805f5260405f205f915090505481565b6001805461068f90610bb2565b80601f01602080910402602001604051908101604052809291908181526020018280546106bb90610bb2565b80156107065780601f106106dd57610100808354040283529160200191610706565b820191905f5260205f20905b8154815290600101906020018083116106e957829003601f168201915b505050505081565b5f8160045f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f2054101561078f576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161078690610c2c565b60405180910390fd5b8160045f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546107db9190610cdf565b925050819055508160045f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f82825461082e9190610d12565b925050819055508273ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef846040516108929190610a7f565b60405180910390a36001905092915050565b6005602052815f5260405f20602052805f5260405f205f91509150505481565b5f81519050919050565b5f82825260208201905092915050565b5f5b838110156108fb5780820151818401526020810190506108e0565b5f8484015250505050565b5f601f19601f8301169050919050565b5f610920826108c4565b61092a81856108ce565b935061093a8185602086016108de565b61094381610906565b840191505092915050565b5f6020820190508181035f8301526109668184610916565b905092915050565b5f80fd5b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f61099b82610972565b9050919050565b6109ab81610991565b81146109b5575f80fd5b50565b5f813590506109c6816109a2565b92915050565b5f819050919050565b6109de816109cc565b81146109e8575f80fd5b50565b5f813590506109f9816109d5565b92915050565b5f8060408385031215610a1557610a1461096e565b5b5f610a22858286016109b8565b9250506020610a33858286016109eb565b9150509250929050565b5f8115159050919050565b610a5181610a3d565b82525050565b5f602082019050610a6a5f830184610a48565b92915050565b610a79816109cc565b82525050565b5f602082019050610a925f830184610a70565b92915050565b5f805f60608486031215610aaf57610aae61096e565b5b5f610abc868287016109b8565b9350506020610acd868287016109b8565b9250506040610ade868287016109eb565b9150509250925092565b5f60ff82169050919050565b610afd81610ae8565b82525050565b5f602082019050610b165f830184610af4565b92915050565b5f60208284031215610b3157610b3061096e565b5b5f610b3e848285016109b8565b91505092915050565b5f8060408385031215610b5d57610b5c61096e565b5b5f610b6a858286016109b8565b9250506020610b7b858286016109b8565b9150509250929050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52602260045260245ffd5b5f6002820490506001821680610bc957607f821691505b602082108103610bdc57610bdc610b85565b5b50919050565b7f496e73756666696369656e742062616c616e63650000000000000000000000005f82015250565b5f610c166014836108ce565b9150610c2182610be2565b602082019050919050565b5f6020820190508181035f830152610c4381610c0a565b9050919050565b7f496e73756666696369656e7420616c6c6f77616e6365000000000000000000005f82015250565b5f610c7e6016836108ce565b9150610c8982610c4a565b602082019050919050565b5f6020820190508181035f830152610cab81610c72565b9050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f610ce9826109cc565b9150610cf4836109cc565b9250828203905081811115610d0c57610d0b610cb2565b5b92915050565b5f610d1c826109cc565b9150610d27836109cc565b9250828201905080821115610d3f57610d3e610cb2565b5b9291505056fea26469706673582212208b562ac4f0f974b2ee612ecf1be3e3c4caa136b06cc2b96ce39f3a0a66c1b9b664736f6c63430008140033';

  // Initial supply: 1,000,000 TEST tokens (1e24 wei)
  const initialSupply = ethers.parseUnits('1000000', 18);
  const constructorArgs = ethers.AbiCoder.defaultAbiCoder().encode(['uint256'], [initialSupply]);
  const deployData = tokenBytecode + constructorArgs.substring(2);

  console.log('Deploying ERC20 token...');
  const deployTx = await new ethers.Wallet(PRIVATE_KEY, new ethers.JsonRpcProvider(L2_RPC_URL)).sendTransaction({
    data: deployData,
    gasLimit: 5_000_000n,
    gasPrice: 1_000_000_000n
  });
  console.log('Deploy tx hash:', deployTx.hash);
  const deployReceipt = await deployTx.wait();
  const tokenAAddress = deployReceipt.contractAddress;
  console.log('Token deployed at:', tokenAAddress);

  const tokenA = new ethers.Contract(tokenAAddress, ERC20Abi, providerA);
  const balanceA = await tokenA.balanceOf(walletA.address);
  console.log('WalletA token balance:', ethers.formatUnits(balanceA, 18), 'TEST');

  // ---- Register token with Native Token Vault ----

  console.log('\n=== REGISTERING TOKEN WITH NATIVE TOKEN VAULT ===');

  const IL2NativeTokenVaultAbi2 = [
    ...IL2NativeTokenVaultAbi,
    'function ensureTokenIsRegistered(address _nativeToken) returns (bytes32)'
  ];

  const nativeTokenVault = new ethers.Contract(
    L2_NATIVE_TOKEN_VAULT_ADDRESS,
    IL2NativeTokenVaultAbi2,
    new ethers.Wallet(PRIVATE_KEY, new ethers.JsonRpcProvider(L2_RPC_URL))
  );

  console.log('Registering token...');
  const registerTx = await nativeTokenVault.ensureTokenIsRegistered(tokenAAddress, {
    gasLimit: 5_000_000n,
    gasPrice: 1_000_000_000n
  });
  console.log('Register tx hash:', registerTx.hash);
  const registerReceipt = await registerTx.wait();
  console.log('Token registered successfully');

  // ---- Approve Native Token Vault to spend tokens ----

  console.log('\n=== APPROVING NATIVE TOKEN VAULT ===');

  // Amount to send: 100 tokens
  const amountToSend = ethers.parseUnits('100', 18);
  console.log('Amount to approve:', ethers.formatUnits(amountToSend, 18), 'TEST');

  const tokenAWithSigner = new ethers.Contract(
    tokenAAddress,
    ERC20Abi,
    new ethers.Wallet(PRIVATE_KEY, new ethers.JsonRpcProvider(L2_RPC_URL))
  );

  const approveTx = await tokenAWithSigner.approve(L2_NATIVE_TOKEN_VAULT_ADDRESS, amountToSend, {
    gasLimit: 100_000n,
    gasPrice: 1_000_000_000n
  });
  console.log('Approve tx hash:', approveTx.hash);
  await approveTx.wait();
  console.log('Approval successful');

  // ---- Send ERC20 tokens via interop ----

  const interopCenter = new zksync.Contract(
    INTEROP_CENTER_ADDRESS,
    IInteropCenterAbi,
    // Must be ethers wallet to avoid sending Era-specific txs
    new ethers.Wallet(PRIVATE_KEY, new ethers.JsonRpcProvider(L2_RPC_URL))
  );

  console.log('\n=== SENDING INTEROP BUNDLE (ERC20 TRANSFER) ===');
  console.log('Amount to send:', ethers.formatUnits(amountToSend, 18), 'TEST');

  // Get the asset ID for the token
  const chainAId = (await providerA.getNetwork()).chainId;
  const assetId = computeAssetId(chainAId, tokenAAddress);
  console.log('Asset ID:', assetId);

  // Build the second bridge calldata
  const secondBridgeCalldata = buildSecondBridgeCalldata(
    assetId,
    amountToSend,
    walletB.address,  // recipient
    ethers.ZeroAddress  // maybeTokenAddress = 0
  );

  // Build the interop call starter with indirectCall attribute
  const indirectCallSelector = ethers.id('indirectCall(uint256)').substring(0, 10);
  const callAttributes = [
    indirectCallSelector + ethers.AbiCoder.defaultAbiCoder().encode(['uint256'], [0n]).substring(2)
  ];

  const calls = [{
    to: formatEvmV1AddressOnly(L2_ASSET_ROUTER_ADDRESS),  // Target the Asset Router
    data: secondBridgeCalldata,
    callAttributes: callAttributes  // Set the indirectCall attribute
  }];

  // Build bundle attributes - unbundler address is required
  const unbundlerAddressSelector = ethers.id('unbundlerAddress(bytes)').substring(0, 10);
  const unbundlerAddressEncoded = ethers.AbiCoder.defaultAbiCoder().encode(
    ['bytes'],
    [formatEvmV1AddressOnly(walletB.address)]  // unbundler address with no chain reference
  );
  const bundleAttributes = [
    unbundlerAddressSelector + unbundlerAddressEncoded.substring(2)  // Remove '0x' from encoded data
  ];

  // Format destination chain ID
  const destinationChainId = formatEvmV1((await providerB.getNetwork()).chainId);

  console.log('Sending bundle...');
  // Skip gas estimation and force send
  const tx = await interopCenter.sendBundle(destinationChainId, calls, bundleAttributes, {
    gasLimit: 5_000_000n,  // Use a high gas limit
    maxFeePerGas: 1_000_000_000n,
    maxPriorityFeePerGas: 0n,
  });

  console.log('Tx hash:', tx.hash);

  const receipt = await tx.wait();
  console.log('Tx mined in block:', receipt.blockNumber);
  console.log(receipt);

  // ---- wait for proof availability ----

  console.log('Waiting for L2 → L1 log proof...');
  await waitForL2ToL1LogProof(
    walletA,
    receipt.blockNumber,
    tx.hash
  );

  const logProof = await walletA.provider.getLogProof(tx.hash, 0);
  console.log(logProof);

  console.log('Proof obtained');
  console.log('L1 batch:', logProof.batch_number);
  console.log('Message index:', logProof.id);

  // ---- execute bundle on second L2 ----

  console.log('Waiting for interop root to become available on second chain...');
    await waitUntilRootBecomesAvailable(
        walletB,
        (await providerA.getNetwork()).chainId,
        logProof.batch_number,
        logProof.root
    );

  // Extract the InteropBundle from the receipt logs
  const iface = new ethers.Interface([
    'event InteropBundleSent(bytes32 l2l1MsgHash, bytes32 interopBundleHash, tuple(bytes1 version, uint256 sourceChainId, uint256 destinationChainId, bytes32 interopBundleSalt, tuple(bytes1 version, bool shadowAccount, address to, address from, uint256 value, bytes data)[] calls, tuple(bytes executionAddress, bytes unbundlerAddress) bundleAttributes) interopBundle)'
  ]);

  let interopBundle = null;
  for (const log of receipt.logs) {
    try {
      const parsed = iface.parseLog({ topics: log.topics, data: log.data });
      if (parsed && parsed.name === 'InteropBundleSent') {
        interopBundle = parsed.args.interopBundle;
        break;
      }
    } catch (e) {
      // Skip logs that don't match
      continue;
    }
  }

  if (!interopBundle) {
    throw new Error('InteropBundleSent event not found in receipt');
  }

  // Encode the bundle
  const encodedBundle = ethers.AbiCoder.defaultAbiCoder().encode(
    ['tuple(bytes1 version, uint256 sourceChainId, uint256 destinationChainId, bytes32 interopBundleSalt, tuple(bytes1 version, bool shadowAccount, address to, address from, uint256 value, bytes data)[] calls, tuple(bytes executionAddress, bytes unbundlerAddress) bundleAttributes)'],
    [interopBundle]
  );

  // InteropCenter prepends BUNDLE_IDENTIFIER (0x01) to the message
  const BUNDLE_IDENTIFIER = '0x01';
  const l2ToL1Message = ethers.concat([BUNDLE_IDENTIFIER, encodedBundle]);

  // Build the MessageInclusionProof
  const messageInclusionProof = {
    chainId: (await providerA.getNetwork()).chainId,
    l1BatchNumber: logProof.batch_number,
    l2MessageIndex: logProof.id,
    message: {
      txNumberInBatch: receipt.index,
      sender: INTEROP_CENTER_ADDRESS,
      data: l2ToL1Message
    },
    proof: logProof.proof
  };

  // Use plain ethers wallet for interop handler call
  const ethersWallet = new ethers.Wallet(PRIVATE_KEY, new ethers.JsonRpcProvider(L2_RPC_URL_SECOND));

  const interopHandler = new ethers.Contract(
    INTEROP_HANDLER_ADDRESS,
    IInteropHandlerAbi,
    ethersWallet
  );

  console.log('\n=== EXECUTE BUNDLE ON SECOND CHAIN ===');
  console.log('Encoded bundle length:', encodedBundle.length);
  console.log('MessageInclusionProof:', JSON.stringify({
    chainId: messageInclusionProof.chainId.toString(),
    l1BatchNumber: messageInclusionProof.l1BatchNumber,
    l2MessageIndex: messageInclusionProof.l2MessageIndex,
    messageSender: messageInclusionProof.message.sender,
    messageTxNumber: messageInclusionProof.message.txNumberInBatch,
    messageDataLength: messageInclusionProof.message.data.length,
    proofLength: messageInclusionProof.proof.length
  }, null, 2));

  // Encode the calldata for executeBundle
  const calldata = interopHandler.interface.encodeFunctionData('executeBundle', [
    encodedBundle,
    messageInclusionProof
  ]);

  console.log('\n=== EXECUTING BUNDLE ON CHAIN B ===');

  // Send executeBundle transaction
  const executeTx = await interopHandler.executeBundle(encodedBundle, messageInclusionProof, {
    gasLimit: 10_000_000n,
    gasPrice: 1_000_000_000n
  });

  console.log('Execute bundle tx hash:', executeTx.hash);

  const executeReceipt = await executeTx.wait();
  console.log('Execute bundle tx mined in block:', executeReceipt.blockNumber);
  console.log('Execute bundle status:', executeReceipt.status === 1 ? '✅ Success' : '❌ Failed');
  console.log('\n=== CHECKING TOKEN BALANCES AFTER EXECUTION ===');

  // Get the wrapped token address on chain B
  const nativeTokenVaultB = new ethers.Contract(
    L2_NATIVE_TOKEN_VAULT_ADDRESS,
    [...IL2NativeTokenVaultAbi, 'function tokenAddress(bytes32 _assetId) view returns (address)'],
    providerB
  );

  try {
    const tokenBAddress = await nativeTokenVaultB.tokenAddress(assetId);
    console.log('Wrapped token address on chain B:', tokenBAddress);

    if (tokenBAddress && tokenBAddress !== ethers.ZeroAddress) {
      const tokenB = new ethers.Contract(tokenBAddress, ERC20Abi, providerB);
      const balanceB = await tokenB.balanceOf(walletB.address);
      console.log('WalletB token balance:', ethers.formatUnits(balanceB, 18), 'TEST');
    } else {
      console.log('Token not yet registered on chain B');
    }
  } catch (e) {
    console.log('Could not check token balance:', e.message);
  }

  console.log('\nExiting without sending transaction from JavaScript.');
}

main().catch((err) => {
  console.error('❌ Error:', err);
  process.exit(1);
});
