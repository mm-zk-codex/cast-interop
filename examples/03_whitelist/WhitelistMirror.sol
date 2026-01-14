// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice Minimal ERC-7786 recipient interface used by zkSync interop.
interface IERC7786Recipient {
    function receiveMessage(
        bytes32 receiveId,
        bytes calldata sender,
        bytes calldata payload
    ) external payable returns (bytes4);
}

/// @title WhitelistMirror
/// @notice Destination-chain contract that mirrors a whitelist from a trusted source-chain contract.
///         It only accepts messages from `trustedSender` (ERC-7930 bytes), set by the owner.
/// @dev No external libraries; suitable for single-command forge compilation.
contract WhitelistMirror is IERC7786Recipient {
    address public owner;

    /// @dev keccak256(trustedSenderBytes), where trustedSenderBytes is ERC-7930 EVMv1 (chainId+address).
    bytes32 public trustedSenderHash;

    mapping(address => bool) public isWhitelisted;

    event TrustedSenderSet(bytes trustedSender, bytes32 trustedSenderHash);
    event WhitelistUpdated(
        uint8 action,
        address indexed account,
        bool isWhitelistedNow
    );

    modifier onlyOwner() {
        require(msg.sender == owner, "ONLY_OWNER");
        _;
    }

    constructor() {
        owner = msg.sender;
    }

    /// @notice Set the trusted interop sender (ERC-7930 bytes for sourceChainId + sourceContractAddress).
    /// @param trustedSender ERC-7930 bytes identifying the only sender allowed to update this mirror.
    function setTrustedSender(bytes calldata trustedSender) external onlyOwner {
        trustedSenderHash = keccak256(trustedSender);
        emit TrustedSenderSet(trustedSender, trustedSenderHash);
    }

    /// @notice ERC-7786 entry point called by InteropHandler during bundle execution.
    /// @dev Payload format: abi.encode(uint8 action, address account)
    ///      action=1 => add, action=2 => remove
    function receiveMessage(
        bytes32 /* receiveId */,
        bytes calldata sender,
        bytes calldata payload
    ) external payable override returns (bytes4) {
        // Check that it is coming from a trusted caller - interop handler.
        require(
            msg.sender == address(0x000000000000000000000000000000000001000d),
            "message must come from interop handler"
        );
        require(keccak256(sender) == trustedSenderHash, "UNTRUSTED_SENDER");

        (uint8 action, address account) = abi.decode(payload, (uint8, address));
        if (action == 1) {
            isWhitelisted[account] = true;
        } else if (action == 2) {
            isWhitelisted[account] = false;
        } else {
            revert("BAD_ACTION");
        }

        emit WhitelistUpdated(action, account, isWhitelisted[account]);
        return this.receiveMessage.selector;
    }
}
