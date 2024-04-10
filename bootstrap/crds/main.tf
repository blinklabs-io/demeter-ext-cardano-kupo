resource "kubernetes_manifest" "customresourcedefinition_kupoports_demeter_run" {
  manifest = {
    "apiVersion" = "apiextensions.k8s.io/v1"
    "kind" = "CustomResourceDefinition"
    "metadata" = {
      "name" = "kupoports.demeter.run"
    }
    "spec" = {
      "group" = "demeter.run"
      "names" = {
        "categories" = []
        "kind" = "KupoPort"
        "plural" = "kupoports"
        "shortNames" = [
          "kpts",
        ]
        "singular" = "kupoport"
      }
      "scope" = "Namespaced"
      "versions" = [
        {
          "additionalPrinterColumns" = [
            {
              "jsonPath" = ".spec.network"
              "name" = "Network"
              "type" = "string"
            },
            {
              "jsonPath" = ".spec.pruneUtxo"
              "name" = "Pruned"
              "type" = "boolean"
            },
            {
              "jsonPath" = ".spec.throughputTier"
              "name" = "Throughput Tier"
              "type" = "string"
            },
            {
              "jsonPath" = ".status.endpointUrl"
              "name" = "Endpoint URL"
              "type" = "string"
            },
            {
              "jsonPath" = ".status.authenticatedEndpointUrl"
              "name" = "Authenticated Endpoint URL"
              "type" = "string"
            },
            {
              "jsonPath" = ".status.authToken"
              "name" = "Auth Token"
              "type" = "string"
            },
          ]
          "name" = "v1alpha1"
          "schema" = {
            "openAPIV3Schema" = {
              "description" = "Auto-generated derived type for KupoPortSpec via `CustomResource`"
              "properties" = {
                "spec" = {
                  "properties" = {
                    "kupoVersion" = {
                      "nullable" = true
                      "type" = "string"
                    }
                    "network" = {
                      "type" = "string"
                    }
                    "operatorVersion" = {
                      "type" = "string"
                    }
                    "pruneUtxo" = {
                      "type" = "boolean"
                    }
                    "throughputTier" = {
                      "type" = "string"
                    }
                  }
                  "required" = [
                    "network",
                    "operatorVersion",
                    "pruneUtxo",
                    "throughputTier",
                  ]
                  "type" = "object"
                }
                "status" = {
                  "nullable" = true
                  "properties" = {
                    "authToken" = {
                      "type" = "string"
                    }
                    "authenticatedEndpointUrl" = {
                      "nullable" = true
                      "type" = "string"
                    }
                    "endpointUrl" = {
                      "type" = "string"
                    }
                  }
                  "required" = [
                    "authToken",
                    "endpointUrl",
                  ]
                  "type" = "object"
                }
              }
              "required" = [
                "spec",
              ]
              "title" = "KupoPort"
              "type" = "object"
            }
          }
          "served" = true
          "storage" = true
          "subresources" = {
            "status" = {}
          }
        },
      ]
    }
  }
}
