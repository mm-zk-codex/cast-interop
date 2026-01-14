// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice Minimal interface for zkSync InteropCenter (system contract).
interface IInteropCenter {
    /// @param recipient ERC-7930 interoperable address bytes (EVMv1 chainId+address)
    /// @param payload Arbitrary bytes payload
    /// @param attributes ERC-7786 attributes (empty for this example)
    /// @return sendId The ERC-7786 sendId (keccak(bundleHash, callIndex))
    function sendMessage(
        bytes calldata recipient,
        bytes calldata payload,
        bytes[] calldata attributes
    ) external payable returns (bytes32 sendId);
}

/// @title WhitelistSource
/// @notice Source-of-truth whitelist on the source chain that syncs updates to a destination chain via interop.
/// @dev No external libraries; suitable for single-command forge compilation.
///      This contract does NOT build ERC-7930 bytes onchain; instead it accepts `destRecipient` as bytes.
///      You can compute it with `cast-interop encode 7930 --chain-id <DEST_ID> --address <MIRROR_ADDR>`.
contract WhitelistSource {
    address public owner;
    address public interopCenter;

    /// @dev ERC-7930 EVMv1 bytes identifying the destination recipient (destChainId + WhitelistMirror address).
    bytes public destRecipient;

    event DestinationSet(address interopCenter, bytes destRecipient);
    event SyncSent(uint8 action, address indexed account, bytes32 sendId);

    modifier onlyOwner() {
        require(msg.sender == owner, "ONLY_OWNER");
        _;
    }

    /// @param _interopCenter InteropCenter system contract (default: 0x...0010010)
    /// @param _destRecipient ERC-7930 bytes for (destination chainId + mirror contract address)
    constructor(address _interopCenter, bytes memory _destRecipient) {
        owner = msg.sender;
        interopCenter = _interopCenter;
        destRecipient = _destRecipient;
        emit DestinationSet(_interopCenter, _destRecipient);
    }

    function setDestination(
        address _interopCenter,
        bytes calldata _destRecipient
    ) external onlyOwner {
        interopCenter = _interopCenter;
        destRecipient = _destRecipient;
        emit DestinationSet(_interopCenter, _destRecipient);
    }

    /// @notice Add an account to the whitelist and sync to destination.
    function add(address account) external onlyOwner returns (bytes32 sendId) {
        sendId = _sync(1, account);
    }

    /// @notice Remove an account from the whitelist and sync to destination.
    function remove(
        address account
    ) external onlyOwner returns (bytes32 sendId) {
        sendId = _sync(2, account);
    }

    function _sync(
        uint8 action,
        address account
    ) internal returns (bytes32 sendId) {
        bytes memory payload = abi.encode(action, account);
        bytes[] memory attrs; // no attributes for this example
        sendId = IInteropCenter(interopCenter).sendMessage(
            destRecipient,
            payload,
            attrs
        );
        emit SyncSent(action, account, sendId);
    }
}
