# Terraform & provider version pins.
#
# CloudSaw ships a single pinned Terraform binary (see Next Steps C2); this
# version constraint is a belt-and-suspenders check that we never apply this
# module with a wildly different Terraform than was tested against.
#
# The AWS provider is the only provider this module uses. Pinning here lets
# Terraform's resolver download a known-good version on `init`.

terraform {
  required_version = ">= 1.6.0, < 2.0.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.40.0, < 6.0.0"
    }
  }
}

# Provider configuration is intentionally empty — CloudSaw injects credentials
# via the AWS SDK provider chain in the spawning process's environment
# (AWS_PROFILE, AWS_REGION, etc.), exactly as it does for STS. No credentials
# are written into this directory.
provider "aws" {}
