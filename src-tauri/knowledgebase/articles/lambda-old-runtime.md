# Lambda function uses a deprecated runtime

## Description
The Lambda function declares a runtime that AWS has marked deprecated or scheduled for deprecation (e.g. Python 3.7, Node.js 14.x, Java 8 on Amazon Linux 1).

## Risk
Deprecated runtimes stop receiving security patches. AWS will eventually block invocation of functions on deprecated runtimes, but updates to ageing functions are operationally risky if they have not been exercised.

## Detection Logic
The scanner compares each function's `Runtime` field to a list of currently-deprecated runtimes maintained by AWS.

## Remediation
Upgrade to a supported runtime (latest Node.js LTS, Python 3.11+, Java 17+). Test the upgrade in staging — newer runtimes often change defaults around concurrency, signatures, and dependencies.

## Terraform Fix
```hcl
resource "aws_lambda_function" "worker" {
  runtime = "python3.11"
  # ...
}
```

## AWS CLI Fix
```sh
aws lambda update-function-configuration \
  --function-name <name> --runtime python3.11
```

## False Positives
Functions that AWS itself supports under "extended support" agreements may still appear here; document the renewal cadence.
