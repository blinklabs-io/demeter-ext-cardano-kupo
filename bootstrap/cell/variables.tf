variable "namespace" {
  type = string
}

variable "salt" {
  type        = string
  description = "Salt used to identify all components as part of the cell. Should be unique between cells."
}

// PVC
variable "volume_name" {
  type = string
}

variable "storage_size" {
  type = string
}

variable "storage_class_name" {
  type = string
}

variable "access_mode" {
  type = string
}

// Instances
variable "instances" {
  type = map(object({
    replicas      = optional(number)
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
}
