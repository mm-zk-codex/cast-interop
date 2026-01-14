# Greeting example

we'll deploy a simple contract to the destination chain, and then send a message to it from the source chain.

## Run & Relay

### Deploying contract on destination chain (3051)

```shell
# Example rich account from zksync os local networks.
export PRIVATE_KEY=0xac1e735be8536c6534bb4f17f06f6afc73b2b5ba84ac2cfb12f7461b20c0bbe3

forge create examples/01_greeting/Greeting.sol:Greeting -r http://localhost:3051 --private-key $PRIVATE_KEY --broadcast


# Deployed to: 0x163CFa0911B9C7166b2608F0E902Fcd341523552
export CONTRACT_ADDR=0x163CFa0911B9C7166b2608F0E902Fcd341523552
```

Check that everything is working as expected

```shell
cast call -r http://localhost:3051 $CONTRACT_ADDR "message()(string)"
# "initialized"
```

### Creating the call from the source chain

Let's send the 'hello' message.


```shell
cast abi-encode "f(string)" "hello" > /tmp/message

cargo run send message --to-chain 6566  --to $CONTRACT_ADDR  --rpc http://localhost:3050  --payload-file /tmp/message  --private-key $PRIVATE_KEY

# tx hash: 0x277d63aeaa0ad66a7b7c7b48ff1a5a0395b543b9a72da62064fc9ce2be6f66dc
# status: true
# sendId: 0x9171a32f9cb0bfe359e4d4c1f6c6440ee5913ea9aa9c091a94fd31c92af1bdc7
export MESSAGE_TX=0x277d63aeaa0ad66a7b7c7b48ff1a5a0395b543b9a72da62064fc9ce2be6f66dc
```

### Relaying the message

Now we have to 'relay', the transaction to the destination chain. This is a permisionless step, and in real production anyone will be able to do this - and there will be available services. But in our local setup, let's do it ourselves.

```shell
cargo run relay --rpc-src http://localhost:3050 --rpc-dest http://localhost:3051 --tx $MESSAGE_TX --private-key $PRIVATE_KEY
# waiting for interop root to become available for 300s...
# interop root available: 0x6a96f27aa19d6432629e4edeb51b8fd177cecfe3c13a478325dc2f07de5c0f3e
# sent tx: 0x20b4a2bb65ec4c9b5636e7920f0171e08d215146f7ec4c7dee537ec30308e1f0
```

### Final checks

Now the message was succesfully run on the destination chain, so let's take a look. You should see 'hello'

```shell
cast call -r http://localhost:3051 $CONTRACT_ADDR "message()(string)"
# "hello"
```
