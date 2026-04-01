// Each cell of the kupo extension containes 1 PVC and an amount of indexers
// (commonly 3, one per network).
locals {
  pvc_name = "db-local-pv-${var.salt}"
}

module "kupo_pvc" {
  source             = "../pvc"
  namespace          = var.namespace
  access_mode        = var.access_mode
  volume_name        = var.volume_name
  storage_class_name = var.storage_class_name
  storage_size       = var.storage_size
  name               = local.pvc_name
}

module "kupo_instances" {
  source   = "../instance"
  for_each = var.instances

  namespace     = var.namespace
  replicas      = coalesce(each.value.replicas, 1)
  image_tag     = each.value.image_tag
  network       = each.value.network
  pruned        = each.value.pruned
  defer_indexes = coalesce(each.value.defer_indexes, false)
  # TODO: This should be probably n2c_endpoint
  n2n_endpoint    = each.value.n2n_endpoint
  db_volume_claim = local.pvc_name
  suffix          = coalesce(each.value.suffix, var.salt)

  resources = coalesce(each.value.resources, {
    limits = {
      cpu    = "1"
      memory = "1Gi"
    }
    requests = {
      cpu    = "500m"
      memory = "1Gi"
    }
  })
  tolerations = coalesce(each.value.tolerations, [
    {
      effect   = "NoSchedule"
      key      = "demeter.run/compute-profile"
      operator = "Equal"
      value    = "disk-intensive"
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
      operator = "Equal"
      value    = "consistent"
    }
  ])
  node_affinity = coalesce(each.value.node_affinity, {
    required_during_scheduling_ignored_during_execution  = {}
    preferred_during_scheduling_ignored_during_execution = []
  })
}
