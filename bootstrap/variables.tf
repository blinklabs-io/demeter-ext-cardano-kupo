variable "namespace" {
  type    = string
  default = "ftr-kupo-v1"
}

variable "networks" {
  type    = list(string)
  default = ["cardano-mainnet", "cardano-preprod", "cardano-preview"]
}

variable "cloud_provider" {
  type    = string
  default = "aws"
}

variable "cluster_issuer" {
  type    = string
  default = "letsencrypt"
}

// Feature
variable "operator_image_tag" {
  type = string
}

variable "operator_tolerations" {
  type = list(object({
    effect   = string
    key      = string
    operator = string
    value    = optional(string)
  }))
  default = [
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-profile"
      operator = "Equal"
      value    = "general-purpose"
    },
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-arch"
      operator = "Equal"
      value    = "x86"
    },
    {
      effect   = "NoSchedule"
      key      = "demeter.run/availability-sla"
      operator = "Exists"
    }
  ]
}

variable "metrics_delay" {
  description = "the inverval for polling metrics data (in seconds)"
  default     = "60"
}

variable "api_key_salt" {
  type = string
}

variable "ingress_class" {
  type = string
}

variable "extension_subdomain" {
  type = string
}

variable "dns_zone" {
  default = "demeter.run"
}

variable "dns_names" {
  description = "Map of network to list of DNS names"
  type        = map(list(string))
  default     = {}
}

// Proxies

// Proxy service annotations
variable "proxy_green_extra_annotations_by_network" {
  description = <<EOT
A map where keys are network names (only those defined in the "networks" variable)
and values are maps of extra annotations for the green proxy service specific
to that network.
EOT
  type        = map(map(string))
  default     = {}
}

variable "proxy_green_image_tag" {
  type = string
}

variable "proxy_green_replicas" {
  type    = number
  default = 1
}

// Proxy service annotations
variable "proxy_blue_extra_annotations_by_network" {
  description = <<EOT
A map where keys are network names (only those defined in the "networks" variable)
and values are maps of extra annotations for the blue proxy service specific
to that network.
EOT
  type        = map(map(string))
  default     = {}
}

variable "proxy_blue_image_tag" {
  type = string
}

variable "proxy_blue_replicas" {
  type    = number
  default = 1
}

variable "proxy_green_tolerations" {
  type = list(object({
    effect   = string
    key      = string
    operator = string
    value    = optional(string)
  }))
  default = [
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-profile"
      operator = "Equal"
      value    = "general-purpose"
    },
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-arch"
      operator = "Equal"
      value    = "x86"
    },
    {
      effect   = "NoSchedule"
      key      = "demeter.run/availability-sla"
      operator = "Exists"
    }
  ]
}

variable "proxy_blue_tolerations" {
  type = list(object({
    effect   = string
    key      = string
    operator = string
    value    = optional(string)
  }))
  default = [
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-profile"
      operator = "Equal"
      value    = "general-purpose"
    },
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-arch"
      operator = "Equal"
      value    = "x86"
    },
    {
      effect   = "NoSchedule"
      key      = "demeter.run/availability-sla"
      operator = "Exists"
    }
  ]
}

variable "proxy_resources" {
  type = object({
    limits = object({
      cpu    = string
      memory = string
    })
    requests = object({
      cpu    = string
      memory = string
    })
  })
  default = {
    limits : {
      cpu : "50m",
      memory : "250Mi"
    }
    requests : {
      cpu : "50m",
      memory : "250Mi"
    }
  }
}

variable "operator_resources" {
  type = object({
    limits = object({
      cpu    = string
      memory = string
    })
    requests = object({
      cpu    = string
      memory = string
    })
  })
  default = {
    limits : {
      cpu : "300m",
      memory : "1Gi"
    }
    requests : {
      cpu : "150m",
      memory : "1Gi"
    }
  }
}

variable "storage_class" {
  type    = string
  default = "nvme"
}

// Cells
variable "cells" {
  type = map(object({
    pvc = object({
      volume_name        = optional(string)
      storage_size       = string
      storage_class_name = string
      access_mode        = string
    })
    instances = map(object({
      image_tag     = string
      network       = string
      pruned        = bool
      defer_indexes = optional(bool)
      n2n_endpoint  = string
      suffix        = optional(string)
      resources = optional(object({
        limits = object({
          cpu    = string
          memory = string
        })
        requests = object({
          cpu    = string
          memory = string
        })
      }))
      tolerations = optional(list(object({
        effect   = string
        key      = string
        operator = string
        value    = optional(string)
      })))
      node_affinity = optional(object({
        required_during_scheduling_ignored_during_execution = optional(
          object({
            node_selector_term = optional(
              list(object({
                match_expressions = optional(
                  list(object({
                    key      = string
                    operator = string
                    values   = list(string)
                  })), []
                )
              })), []
            )
          }), {}
        )
        preferred_during_scheduling_ignored_during_execution = optional(
          list(object({
            weight = number
            preference = object({
              match_expressions = optional(
                list(object({
                  key      = string
                  operator = string
                  values   = list(string)
                })), []
              )
              match_fields = optional(
                list(object({
                  key      = string
                  operator = string
                  values   = list(string)
                })), []
              )
            })
          })), []
        )
      }))
    }))
  }))
}
