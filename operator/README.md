# Kupo Operator

This operator will create a key into the CRD to allow Kupo to be accessed externally.

## Environment

| Key                  | Value                         |
| -------------------- | ----------------------------- |
| ADDR                 | 0.0.0.0:5000                  |
| EXTENSION_SUBDOMAIN  | kupo-m1                       |
| API_KEY_SALT         | kupo-salt                     |
| METRICS_DELAY        | 40                            |
| PROMETHEUS_URL       |                               |
| DEFAULT_KUPO_VERSION | 2                             |

## Port CRD

To define a new port, a new k8s manifest needs to be created and set the configuration values.

```yml
apiVersion: demeter.run/v1alpha1
kind: KupoPort
metadata:
  name: kupo-port-a123ds
  namespace: prj-mainnet-test
spec:
  operatorVersion: "1"
  kupoVersion: "v1"
  network: mainnet
  pruneUtxo: false
  throughputTier: "0"
```

`network`: The Kupo network the port will consume.
`throughputTier`: The tier to limit how many requests the port can do. The tiers will be configured in *tiers.toml* on the proxy.

## Commands

To generate the CRD will need to execute crdgen

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
