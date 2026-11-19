# Kupo Proxy

This proxy will allow Kupo to be accessed externally.

## Environment

| Key              | Value                   |
| ---------------- | ----------------------- |
| PROXY_ADDR       | 0.0.0.0:5000            |
| PROXY_NAMESPACE  |                         |
| PROMETHEUS_ADDR  | 0.0.0.0:9090            |
| SSL_CRT_PATH     | /localhost.crt          |
| SSL_KEY_PATH     | /localhost.key          |
| KUPO_INSTANCES   | JSON map of network to internal k8s dns |
| HEALTH_NETWORK   | health upstream network, defaults to cardano-mainnet |
| PROXY_TIERS_PATH | path of tiers toml file |

## Rate limit
To define rate limits, it's necessary to create a file with the limiters available that the ports can use. The request limit of each tier can be configured using `s = second`, `m = minute`, `h = hour` and `d = day` eg: `5s` bucket of 5 seconds.

```toml
[[tiers]]
name = "tier0"
[[tiers.rates]]
interval = "1s"
limit = 1
[[tiers.rates]]
interval = "1m"
limit = 10
[[tiers.rates]]
interval = "1h"
limit = 100
[[tiers.rates]]
interval = "1d"
limit = 1000

[[tiers]]
name = "tier1"
[[tiers.rates]]
interval = "5s"
limit = 10
```

after configuring, the file path must be set at the env `PROXY_TIERS_PATH`.


## Commands

To generate the CRD will need to execute `crdgen`

```bash
cargo run --bin=crdgen
```

and execute the operator

```bash
cargo run
```

## Metrics

to collect metrics for Prometheus, an HTTP API will enable the route /metrics.

```
/metrics
```

## TLS certificate renewal

The proxy loads `SSL_CRT_PATH` and `SSL_KEY_PATH` at startup, then polls them
every 60 seconds. Unchanged contents skip certificate parsing and validation. A valid replacement is used for new TLS handshakes without
restarting the process. Existing connections continue normally.

Certificate validity dates, adjacent issuer signatures, and the matching private
key are validated before the
active pair is replaced. Missing, malformed, or mismatched files leave the
previous pair active and produce an error log; a later valid update is retried.
An invalid initial pair prevents startup. Retaining a certificate does not extend
its expiry: monitor reload errors and the certificate served by the listener.

Kubernetes Secret volumes are supported, including atomic `..data` symlink
replacement. Mount the Secret directory rather than individual files through
`subPath`, which does not receive Secret updates. Detection follows the kubelet's
projection update plus the polling interval. No Secret API permissions or restart
controller are required.

Run `cargo test --locked -p proxy` for renewal, failed-reload recovery, and
Kubernetes-style symlink replacement tests with real TLS handshakes.
