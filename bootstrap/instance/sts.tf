locals {
  instance_tmp  = var.pruned ? "kupo-${trimprefix(var.network, "cardano-")}-pruned" : "kupo-${trimprefix(var.network, "cardano-")}"
  instance_name = var.suffix != "" ? "${local.instance_tmp}-${var.suffix}" : local.instance_tmp
  base_args = [
    "--workdir",
    "/db/${var.namespace}/${local.instance_name}",
    "--host",
    "0.0.0.0",
    "--node-socket",
    "/ipc/node.socket",
    "--node-config",
    "/config/config.json",
    "--match",
    "*",
    "--since",
    "origin",
  ]
  temp_args = var.pruned ? concat(local.base_args, ["--prune-utxo"]) : local.base_args
  args      = var.defer_indexes ? concat(local.temp_args, ["--defer-db-indexes"]) : local.temp_args
}

resource "kubernetes_stateful_set_v1" "kupo" {
  wait_for_rollout = "false"
  metadata {
    name      = local.instance_name
    namespace = var.namespace
    labels = {
      "demeter.run/kind"                = "KupoInstance"
      "cardano.demeter.run/network"     = var.network
      "cardano.demeter.run/kupo-pruned" = var.pruned ? "true" : "false"
    }
  }
  spec {
    replicas     = 1
    service_name = "kupo"
    selector {
      match_labels = {
        "demeter.run/instance"            = local.instance_name
        "cardano.demeter.run/network"     = var.network
        "cardano.demeter.run/kupo-pruned" = var.pruned ? "true" : "false"
      }
    }
    template {
      metadata {
        labels = {
          "demeter.run/instance"            = local.instance_name
          "cardano.demeter.run/network"     = var.network
          "cardano.demeter.run/kupo-pruned" = var.pruned ? "true" : "false"
        }
      }
      spec {
        security_context {
          fs_group = 1000
        }

        dynamic "affinity" {
          for_each = (
            var.node_affinity != null &&
            (
              try(length(var.node_affinity.required_during_scheduling_ignored_during_execution.node_selector_term), 0) > 0 ||
              try(length(var.node_affinity.preferred_during_scheduling_ignored_during_execution), 0) > 0
            )
          ) ? [var.node_affinity] : []
          content {
            node_affinity {
              dynamic "required_during_scheduling_ignored_during_execution" {
                for_each = (
                  var.node_affinity.required_during_scheduling_ignored_during_execution != null &&
                  length(var.node_affinity.required_during_scheduling_ignored_during_execution.node_selector_term) > 0
                ) ? [var.node_affinity.required_during_scheduling_ignored_during_execution] : []
                content {
                  dynamic "node_selector_term" {
                    for_each = required_during_scheduling_ignored_during_execution.value.node_selector_term
                    content {
                      dynamic "match_expressions" {
                        for_each = length(node_selector_term.value.match_expressions) > 0 ? node_selector_term.value.match_expressions : []
                        content {
                          key      = match_expressions.value.key
                          operator = match_expressions.value.operator
                          values   = match_expressions.value.values
                        }
                      }
                    }
                  }
                }
              }
              dynamic "preferred_during_scheduling_ignored_during_execution" {
                for_each = (
                  var.node_affinity.preferred_during_scheduling_ignored_during_execution != null &&
                  length(var.node_affinity.preferred_during_scheduling_ignored_during_execution) > 0
                ) ? var.node_affinity.preferred_during_scheduling_ignored_during_execution : []
                content {
                  weight = preferred_during_scheduling_ignored_during_execution.value.weight

                  dynamic "preference" {
                    for_each = (
                      length(preferred_during_scheduling_ignored_during_execution.value.preference.match_expressions) > 0 ||
                      length(preferred_during_scheduling_ignored_during_execution.value.preference.match_fields) > 0
                    ) ? [preferred_during_scheduling_ignored_during_execution.value.preference] : []
                    content {
                      dynamic "match_expressions" {
                        for_each = length(preference.value.match_expressions) > 0 ? preference.value.match_expressions : []
                        content {
                          key      = match_expressions.value.key
                          operator = match_expressions.value.operator
                          values   = match_expressions.value.values
                        }
                      }
                      dynamic "match_fields" {
                        for_each = length(preference.value.match_fields) > 0 ? preference.value.match_fields : []
                        content {
                          key      = match_fields.value.key
                          operator = match_fields.value.operator
                          values   = match_fields.value.values
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }

        container {
          name              = "main"
          image             = "ghcr.io/demeter-run/ext-cardano-kupo-instance:${var.image_tag}"
          image_pull_policy = "Always"
          args              = local.args

          port {
            container_port = 1442
            name           = "http"
            protocol       = "TCP"
          }

          resources {
            limits = {
              cpu    = var.resources.limits.cpu
              memory = var.resources.limits.memory
            }
            requests = {
              cpu    = var.resources.requests.cpu
              memory = var.resources.requests.memory
            }
          }

          env {
            name  = "GHCRTS"
            value = "-N8"
          }

          volume_mount {
            mount_path = "/db"
            name       = "db"
          }

          volume_mount {
            mount_path = "/config"
            name       = "node-config"
          }

          volume_mount {
            mount_path = "/ipc"
            name       = "cardanoipc"
          }

          readiness_probe {
            exec {
              command = ["/bin/sh", "-c", <<-EOF
                URL='http://localhost:1442/health';
                METRICS=$(wget -qO- --header="Accept: text/plain" $URL);
                NODE_TIP=$(echo "$METRICS" | grep 'kupo_most_recent_node_tip' | awk '{print $NF}' | tr -d '"');
                CHECKPOINT=$(echo "$METRICS" | grep 'kupo_most_recent_checkpoint' | awk '{print $NF}' | tr -d '"');
                if [ -z "$NODE_TIP" ] || [ -z "$CHECKPOINT" ]; then
                  echo 'Error: NODE_TIP or CHECKPOINT is null.';
                  exit 1;
                fi;
                if [ "$NODE_TIP" = '0' ] || [ "$CHECKPOINT" = '0' ]; then
                  echo 'Error: NODE_TIP or CHECKPOINT is 0.';
                  exit 1;
                fi;
                if [ "$NODE_TIP" = "$CHECKPOINT" ]; then
                  exit 0;
                else
                  exit 1;
                fi
              EOF
              ]
            }

            initial_delay_seconds = 5
            period_seconds        = 30
          }
        }

        container {
          name  = "socat"
          image = "alpine/socat:latest"
          args = [
            "-d",
            "UNIX-LISTEN:/ipc/node.socket,fork,reuseaddr,unlink-early",
            "TCP:${var.n2n_endpoint}",
          ]
          volume_mount {
            mount_path = "/ipc"
            name       = "cardanoipc"
          }
        }

        volume {
          name = "cardanoipc"
          empty_dir {}
        }

        volume {
          name = "node-config"
          config_map {
            name = "configs-${var.network}"
          }
        }

        volume {
          name = "db"
          persistent_volume_claim {
            claim_name = var.db_volume_claim
          }
        }

        dynamic "toleration" {
          for_each = var.tolerations
          content {
            effect   = toleration.value.effect
            key      = toleration.value.key
            operator = toleration.value.operator
            value    = try(toleration.value["value"], null)
          }
        }
      }
    }
  }
}
