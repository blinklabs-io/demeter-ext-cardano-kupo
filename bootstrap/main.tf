resource "kubernetes_namespace" "namespace" {
  metadata {
    name = var.namespace
  }
}

module "kupo_feature" {
  depends_on          = [kubernetes_namespace.namespace]
  source              = "./feature"
  operator_image_tag  = var.operator_image_tag
  ingress_class       = var.ingress_class
  extension_subdomain = var.extension_subdomain
  dns_zone            = var.dns_zone
  api_key_salt        = var.api_key_salt
  namespace           = var.namespace
  resources           = var.operator_resources
  tolerations         = var.operator_tolerations
}

module "kupo_configs" {
  source   = "./configs"
  for_each = { for network in var.networks : "${network}" => network }

  namespace = var.namespace
  network   = each.value
}

module "kupo_services" {
  depends_on = [kubernetes_namespace.namespace]
  for_each   = { for network in var.networks : "${network}" => network }
  source     = "./service"

  namespace = var.namespace
  network   = each.value
  prune     = true
}

module "kupo_services_non_pruned" {
  depends_on = [kubernetes_namespace.namespace]
  for_each   = { for network in var.networks : "${network}" => network }
  source     = "./service"

  namespace = var.namespace
  network   = each.value
  prune     = false
}

// blue (once we have a green, we can update its name to proxy-blue)
module "kupo_proxies_blue" {
  depends_on = [kubernetes_namespace.namespace]
  for_each   = { for network in var.networks : "${network}" => network }
  source     = "./proxy"

  network           = each.value
  cloud_provider    = var.cloud_provider
  cluster_issuer    = var.cluster_issuer
  namespace         = var.namespace
  replicas          = var.proxy_blue_replicas
  extension_name    = var.extension_subdomain
  extra_annotations = lookup(var.proxy_blue_extra_annotations_by_network, each.value, {})
  proxy_image_tag   = var.proxy_blue_image_tag
  resources         = var.proxy_resources
  environment       = "blue"
  name              = "proxy-blue-${each.value}"
  tolerations       = var.proxy_blue_tolerations
  cert_secret_name  = "proxy-blue-${each.value}-wildcard-tls"
  dns_names = lookup(var.dns_names, each.value, [
    "${each.value}-v2.${var.extension_subdomain}.${var.dns_zone}",
    "*.${each.value}-v2.${var.extension_subdomain}.${var.dns_zone}"
  ])
}

module "kupo_proxies_green" {
  for_each   = { for network in var.networks : "${network}" => network }
  depends_on = [kubernetes_namespace.namespace]
  source     = "./proxy"

  network           = each.value
  cloud_provider    = var.cloud_provider
  cluster_issuer    = var.cluster_issuer
  namespace         = var.namespace
  replicas          = var.proxy_green_replicas
  extension_name    = var.extension_subdomain
  extra_annotations = lookup(var.proxy_green_extra_annotations_by_network, each.value, {})
  proxy_image_tag   = var.proxy_green_image_tag
  resources         = var.proxy_resources
  environment       = "green"
  name              = "proxy-green-${each.value}"
  tolerations       = var.proxy_green_tolerations
  cert_secret_name  = "proxy-green-${each.value}-wildcard-tls"
  dns_names = lookup(var.dns_names, each.value, [
    "${each.value}-v2.${var.extension_subdomain}.${var.dns_zone}",
    "*.${each.value}-v2.${var.extension_subdomain}.${var.dns_zone}"
  ])
}

module "kupo_cells" {
  depends_on = [module.kupo_feature, module.kupo_configs]
  for_each   = var.cells
  source     = "./cell"

  namespace = var.namespace
  salt      = each.key

  // PVC
  volume_name        = each.value.pvc.volume_name
  storage_size       = each.value.pvc.storage_size
  storage_class_name = each.value.pvc.storage_class_name
  access_mode        = each.value.pvc.access_mode

  // Instances
  instances = each.value.instances
}
