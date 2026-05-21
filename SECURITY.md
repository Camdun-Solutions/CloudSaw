# Security Policy

CloudSaw runs entirely on the user's machine, never transmits scan data
externally, and treats the AWS access path as security-critical. Reports of
vulnerabilities are taken seriously and acknowledged.

## Reporting a vulnerability

Email **security@cloud-saw.com** with:

  - a short description of the issue,
  - reproduction steps if you have them, and
  - any proof-of-concept code or screenshots.

If you would like to encrypt your report, request the project's PGP public
key in your initial email; we will respond with it. (Once the key is
published, its fingerprint will replace this paragraph.)

Please **do not** open a public GitHub issue for security reports.

## Disclosure timeline

  - **Acknowledgment:** within 3 business days of receipt.
  - **Initial assessment:** within 10 business days.
  - **Coordinated disclosure window:** up to **90 days** from
    acknowledgment, or sooner if a patched release is published.
  - **Credit:** reporters are credited in release notes and on the
    Security page of cloud-saw.com unless they request otherwise.

## Out of scope

  - Issues in third-party software we bundle (ScoutSuite, Terraform) that are
    not specific to CloudSaw's use of them — please report those upstream and
    let us know so we can update the pinned version.
  - Findings that require an attacker to already have local admin rights on
    the user's machine.

## Supported versions

Security fixes target the latest CalVer release and, when feasible, are
backported to the previous quarter's release. Older releases are not
supported.
