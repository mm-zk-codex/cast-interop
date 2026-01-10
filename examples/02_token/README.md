# Token example

We'll do a simple ERC20 token transfer between chains.

There are two main ways of doing it:
* using the Elastic network to handle the bridging (easy)
* or doing the bridging on your own (harder)

The first way will be described below.

If you're interested in your own custom bridging, please look at the NFT example.


### Deploying the ERC20 contract on the first chain

```shell
# rich wallet, that we'll use for experiments
export PRIVATE_KEY=0xac1e735be8536c6534bb4f17f06f6afc73b2b5ba84ac2cfb12f7461b20c0bbe3
export ADDRESS=0xa61464658AfeAf65CccaaFD3a512b69A83B77618

forge create examples/02_token/ERC20.sol:ERC20 -r http://localhost:3050 --private-key $PRIVATE_KEY --broadcast
# Deployed to: 0xdC5503A345A584382EF9c8dcB015e5eC095c730c
export TOKEN_ADDRESS=0xdC5503A345A584382EF9c8dcB015e5eC095c730c
```

Confirm the balance

```shell
cast call -r http://localhost:3050 $TOKEN_ADDRESS 'balanceOf(address)(uint256)' $ADDRESS
```


### Your token on other chains

You can register your token within the system, getting automatic bridge contracts deployments on every L2 chain in the ecosystem.

```shell
cargo run token info --token $TOKEN_ADDRESS --rpc-src http://localhost:3050 --rpc-dest http://localhost:3051

#
#source chainId: 6565
#destination chainId: 6566
#token (source): 0xdc5503a345a584382ef9c8dcb015e5ec095c730c
#native token vault: 0x0000000000000000000000000000000000010004
#assetId: 0x5d0106cd0970e0dee68b05de05188b6ef284c9a0ebe9f9a326021c11cd932a39 -- this is globally unique ID of your token.
#wrapped token (dest): 0x0000000000000000000000000000000000000000 -- ok, seems it is not registered on the destination chain yet.
```

We can do the registration, by simply sending one unit of token over.

### Sending token to other chain

Let's send 3 units of token to the other chain.

```shell
cargo run token send --token $TOKEN_ADDRESS --to $ADDRESS --rpc-src http://localhost:3050   --rpc-dest http://localhost:3051 --private-key $PRIVATE_KEY --amount-wei 3

# ... lots of logs
# Waiting for finalized block on source...
# Waiting for log proof on source...
# destination balance: 0.000000000000000003
# destination balance (raw): 3
```

Let's see the token info:

```shell
cargo run token info --token $TOKEN_ADDRESS --rpc-src http://localhost:3050 --rpc-dest http://localhost:3051

# ...
# wrapped token (dest): 0x989bd5661de9a733db8599e9307625ee910768b1
```

And now you can see that the 'destination' wrapped token contract was succesfully deployed.



### Checking the balances

There are 2 ways how you can check the balance: 'token balance' or simply doing a cast call for 'balanceOf'

```shell
cargo run token balance --token $TOKEN_ADDRESS --to $ADDRESS --rpc-src http://localhost:3050   --rpc-dest http://localhost:3051
# wrapped token (dest): 0x989bd5661de9a733db8599e9307625ee910768b1
# balance: 0.000000000000000003
# balance (raw): 3
# decimals: 18
export WRAPPED_TOKEN=0x989bd5661de9a733db8599e9307625ee910768b1
```

or 

```shell
cast call -r http://localhost:3051 $WRAPPED_TOKEN 'balanceOf(address)(uint256)' $ADDRESS
```
