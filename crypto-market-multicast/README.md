## server
```shell
cargo run --package crypto-market-multicast -- server 230.0.0.1 8080 ipc:///tmp/binance_spot_l2_topk_BTCUSDT.ipc
```

## client
```shell
cargo run --package crypto-market-multicast -- server 230.0.0.1 8080
```
