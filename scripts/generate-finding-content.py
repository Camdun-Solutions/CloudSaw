#!/usr/bin/env python3
"""
PR #83 — generate per-finding description / risk text + comprehensive
compliance mappings for every ScoutSuite rule the app surfaces.

Inputs:
  src-tauri/knowledgebase/scoutsuite_metadata.json
    The current set of 117 rule_keys. We only mine the keys; the
    upstream `remediation` / `references` / `compliance` fields the
    file already carries are left untouched in the output.
  src-tauri/knowledgebase/mappings.json
    The current 4-framework mapping bundle. Existing entries are
    PRESERVED on collision (hand-authored mappings win); generated
    entries fill the gaps for the other 113 findings.

Outputs (overwritten):
  src-tauri/knowledgebase/scoutsuite_metadata.json
    Now carries `name`, `description`, `risk` fields per rule.
    `remediation` / `references` / `compliance` are passed through.
  src-tauri/knowledgebase/mappings.json
    Framework registry extended with `pcidss`; every rule has a
    mapping entry covering soc2 / iso27001 / hipaa / nist / pcidss.

Categorization strategy: each rule_key is matched against an ordered
list of (predicate, category) tuples; the first matching category wins.
Categories define a single template that supplies description, risk,
and a per-framework control set. Per-rule slot fills (the human label,
a verb, the target attribute) come from a small parser that splits the
rule_key into structured fields.

Re-running this script with the same inputs is byte-stable. The script
is committed alongside the JSON so the next time the rule set changes
the regeneration path is deterministic and reviewable.
"""

from __future__ import annotations

import json
import os
import re
import sys
from collections import OrderedDict
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parent.parent
METADATA_PATH = ROOT / "src-tauri" / "knowledgebase" / "scoutsuite_metadata.json"
MAPPINGS_PATH = ROOT / "src-tauri" / "knowledgebase" / "mappings.json"


# ----------------------------------------------------------------------
# Framework control library. Each category template lists the control
# IDs that apply for that category in each of the five generated
# frameworks. CIS stays scoutsuite-derived, so it is not in this map.
# ----------------------------------------------------------------------

# Controls referenced often enough to factor out. The dict key is what
# the templates use; the value is (control_id, title).
SOC2 = {
    "CC6.1": ("CC6.1", "Logical access security software, infrastructure, and architectures"),
    "CC6.2": ("CC6.2", "User registration and authorization"),
    "CC6.3": ("CC6.3", "Access removal and modification"),
    "CC6.6": ("CC6.6", "Logical access security measures to protect against threats"),
    "CC6.7": ("CC6.7", "Restriction and protection of data transmission"),
    "CC6.8": ("CC6.8", "Prevention and detection of unauthorized software"),
    "CC7.1": ("CC7.1", "Detection of configuration changes"),
    "CC7.2": ("CC7.2", "Monitoring for anomalies"),
    "CC7.3": ("CC7.3", "Incident response evaluation"),
    "CC7.4": ("CC7.4", "Incident response and recovery"),
    "CC8.1": ("CC8.1", "Authorized changes to infrastructure and data"),
    "A1.2": ("A1.2", "Backup, redundancy, and recovery"),
    "C1.1": ("C1.1", "Protection of confidential information"),
}

ISO = {
    "A.5.15": ("A.5.15", "Access control"),
    "A.5.16": ("A.5.16", "Identity management"),
    "A.5.17": ("A.5.17", "Authentication information"),
    "A.5.18": ("A.5.18", "Access rights"),
    "A.5.23": ("A.5.23", "Information security for use of cloud services"),
    "A.5.30": ("A.5.30", "ICT readiness for business continuity"),
    "A.8.2": ("A.8.2", "Privileged access rights"),
    "A.8.3": ("A.8.3", "Information access restriction"),
    "A.8.5": ("A.8.5", "Secure authentication"),
    "A.8.7": ("A.8.7", "Protection against malware"),
    "A.8.8": ("A.8.8", "Management of technical vulnerabilities"),
    "A.8.9": ("A.8.9", "Configuration management"),
    "A.8.13": ("A.8.13", "Information backup"),
    "A.8.15": ("A.8.15", "Logging"),
    "A.8.16": ("A.8.16", "Monitoring activities"),
    "A.8.20": ("A.8.20", "Network security"),
    "A.8.21": ("A.8.21", "Security of network services"),
    "A.8.22": ("A.8.22", "Segregation of networks"),
    "A.8.23": ("A.8.23", "Web filtering"),
    "A.8.24": ("A.8.24", "Use of cryptography"),
    "A.8.25": ("A.8.25", "Secure development life cycle"),
    "A.8.27": ("A.8.27", "Secure system architecture and engineering principles"),
    "A.8.28": ("A.8.28", "Secure coding"),
    "A.8.32": ("A.8.32", "Change management"),
}

HIPAA = {
    "164.308(a)(1)": ("§164.308(a)(1)", "Security management process"),
    "164.308(a)(3)": ("§164.308(a)(3)", "Workforce security"),
    "164.308(a)(4)": ("§164.308(a)(4)", "Information access management"),
    "164.308(a)(5)": ("§164.308(a)(5)", "Security awareness and training"),
    "164.308(a)(5)(ii)(C)": ("§164.308(a)(5)(ii)(C)", "Log-in monitoring"),
    "164.308(a)(5)(ii)(D)": ("§164.308(a)(5)(ii)(D)", "Password management"),
    "164.308(a)(6)": ("§164.308(a)(6)", "Security incident procedures"),
    "164.308(a)(7)": ("§164.308(a)(7)", "Contingency plan"),
    "164.312(a)(1)": ("§164.312(a)(1)", "Access control"),
    "164.312(a)(2)(i)": ("§164.312(a)(2)(i)", "Unique user identification"),
    "164.312(a)(2)(iii)": ("§164.312(a)(2)(iii)", "Automatic logoff"),
    "164.312(a)(2)(iv)": ("§164.312(a)(2)(iv)", "Encryption and decryption"),
    "164.312(b)": ("§164.312(b)", "Audit controls"),
    "164.312(c)(1)": ("§164.312(c)(1)", "Integrity"),
    "164.312(d)": ("§164.312(d)", "Person or entity authentication"),
    "164.312(e)(1)": ("§164.312(e)(1)", "Transmission security"),
    "164.312(e)(2)(i)": ("§164.312(e)(2)(i)", "Integrity controls"),
    "164.312(e)(2)(ii)": ("§164.312(e)(2)(ii)", "Encryption (transmission)"),
}

NIST = {
    "AC-2": ("AC-2", "Account management"),
    "AC-3": ("AC-3", "Access enforcement"),
    "AC-5": ("AC-5", "Separation of duties"),
    "AC-6": ("AC-6", "Least privilege"),
    "AC-7": ("AC-7", "Unsuccessful logon attempts"),
    "AC-17": ("AC-17", "Remote access"),
    "AT-2": ("AT-2", "Literacy training and awareness"),
    "AU-2": ("AU-2", "Event logging"),
    "AU-3": ("AU-3", "Content of audit records"),
    "AU-6": ("AU-6", "Audit record review, analysis, and reporting"),
    "AU-9": ("AU-9", "Protection of audit information"),
    "AU-12": ("AU-12", "Audit record generation"),
    "CM-2": ("CM-2", "Baseline configuration"),
    "CM-6": ("CM-6", "Configuration settings"),
    "CM-7": ("CM-7", "Least functionality"),
    "CP-9": ("CP-9", "System backup"),
    "CP-10": ("CP-10", "System recovery and reconstitution"),
    "IA-2": ("IA-2", "Identification and authentication (organizational users)"),
    "IA-2(1)": ("IA-2(1)", "Multi-factor authentication to privileged accounts"),
    "IA-5": ("IA-5", "Authenticator management"),
    "IA-5(1)": ("IA-5(1)", "Password-based authentication"),
    "SC-7": ("SC-7", "Boundary protection"),
    "SC-8": ("SC-8", "Transmission confidentiality and integrity"),
    "SC-12": ("SC-12", "Cryptographic key establishment and management"),
    "SC-13": ("SC-13", "Cryptographic protection"),
    "SC-23": ("SC-23", "Session authenticity"),
    "SC-28": ("SC-28", "Protection of information at rest"),
    "SI-2": ("SI-2", "Flaw remediation"),
    "SI-4": ("SI-4", "System monitoring"),
}

PCI = {
    "REQ-1": ("REQ-1", "Install and maintain network security controls"),
    "REQ-2": ("REQ-2", "Apply secure configurations to all system components"),
    "REQ-3": ("REQ-3", "Protect stored account data"),
    "REQ-3.5": ("REQ-3.5", "Render PAN unreadable"),
    "REQ-4": ("REQ-4", "Protect cardholder data with strong cryptography during transmission"),
    "REQ-5": ("REQ-5", "Protect systems from malicious software"),
    "REQ-6": ("REQ-6", "Develop and maintain secure systems and software"),
    "REQ-6.3": ("REQ-6.3", "Identify and address security vulnerabilities"),
    "REQ-7": ("REQ-7", "Restrict access to system components by business need-to-know"),
    "REQ-8": ("REQ-8", "Identify users and authenticate access"),
    "REQ-8.2": ("REQ-8.2", "User identification and credential management"),
    "REQ-8.3": ("REQ-8.3", "Strong authentication for users"),
    "REQ-8.4": ("REQ-8.4", "Multi-factor authentication for access"),
    "REQ-10": ("REQ-10", "Log and monitor all access to system components"),
    "REQ-10.2": ("REQ-10.2", "Implement audit logs"),
    "REQ-11": ("REQ-11", "Test security of systems and networks regularly"),
    "REQ-12": ("REQ-12", "Support information security with policies and programs"),
}


# ----------------------------------------------------------------------
# Category catalog. Each category maps to:
#   - description_template: f-string with {label} (human rule name)
#   - risk_template: f-string with {label}
#   - frameworks: dict[fw_id] -> list[control_key]
#
# The templates are short on purpose — the user wants the panel to
# render Description / Risk / Remediation as digestible paragraphs, not
# essays. Hand-authored bundled articles in src-tauri/knowledgebase/
# articles/ still take precedence for any rule they cover, so the
# generated text is the floor, not the ceiling.
# ----------------------------------------------------------------------

CATEGORIES = {
    "password-policy": {
        "description": (
            "The IAM account password policy does not enforce {label}. AWS evaluates the policy "
            "against every console password set or reset by a human user; gaps in the policy "
            "translate directly into weaker credentials sitting in production."
        ),
        "risk": (
            "Weakened password policy makes credential-guessing attacks (password spray, "
            "dictionary lookup, reuse across services) materially cheaper. Compromised "
            "console credentials are the most common starting point for AWS account "
            "takeover incidents."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.6"],
            "iso27001": ["A.5.17", "A.8.5"],
            "hipaa": ["164.308(a)(5)(ii)(D)", "164.312(d)"],
            "nist": ["IA-5", "IA-5(1)"],
            "pcidss": ["REQ-8.2", "REQ-8.3"],
        },
    },
    "mfa": {
        "description": (
            "{label}. AWS does not require multi-factor authentication on a console- or API-"
            "facing principal by default — it has to be enforced via IAM policy, Identity Center, "
            "or an organization-wide SCP. The flagged principal is currently protected by a "
            "single factor."
        ),
        "risk": (
            "Single-factor authentication on an AWS principal is one phishing email or one "
            "leaked-password-database hit away from a full takeover. AWS incident response "
            "data consistently shows missing MFA as the top initial-access vector for cloud "
            "account compromise."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.6"],
            "iso27001": ["A.5.17", "A.8.5", "A.8.2"],
            "hipaa": ["164.312(d)", "164.308(a)(4)"],
            "nist": ["IA-2", "IA-2(1)", "IA-5"],
            "pcidss": ["REQ-8.3", "REQ-8.4"],
        },
    },
    "root-account": {
        "description": (
            "{label}. The AWS root account holds permanent, unrevocable authority over the "
            "billing, security, and resource state of the account; AWS guidance is to lock it "
            "down to a hardware MFA-protected break-glass identity and use IAM (or Identity "
            "Center) for routine work."
        ),
        "risk": (
            "Compromise of the root account is essentially unrecoverable — root credentials "
            "can disable CloudTrail, rotate keys, and create new admins faster than any "
            "automated control can react. Every active root credential, every recent root "
            "sign-in, and every missing root MFA increases the blast radius of one phish."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.2", "CC6.3"],
            "iso27001": ["A.5.15", "A.5.16", "A.8.2"],
            "hipaa": ["164.308(a)(3)", "164.308(a)(4)", "164.312(a)(1)"],
            "nist": ["AC-2", "AC-6", "IA-2(1)"],
            "pcidss": ["REQ-7", "REQ-8.3", "REQ-8.4"],
        },
    },
    "iam-privilege": {
        "description": (
            "{label}. The flagged policy grants more authority than the workload appears to "
            "need (full-action wildcards, NotAction-style allow lists, or unbounded resource "
            "selectors). The principle of least privilege requires the policy be narrowed to "
            "the specific actions and resources the workload actually uses."
        ),
        "risk": (
            "Over-broad IAM permissions turn any single compromised credential into a foothold "
            "for lateral movement across the account. Attackers routinely enumerate attached "
            "policies and pivot from one over-permissioned role to another to reach data, "
            "secrets, or billing."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.3", "CC8.1"],
            "iso27001": ["A.5.15", "A.5.18", "A.8.2", "A.8.3"],
            "hipaa": ["164.308(a)(3)", "164.308(a)(4)", "164.312(a)(1)"],
            "nist": ["AC-2", "AC-3", "AC-6", "CM-7"],
            "pcidss": ["REQ-7", "REQ-2"],
        },
    },
    "iam-inline-policy": {
        "description": (
            "{label}. Inline IAM policies are harder to audit and reuse than managed policies; "
            "AWS guidance is to factor reusable rules into customer-managed policies, attach "
            "them by name, and reserve inline policies for genuinely one-off cases."
        ),
        "risk": (
            "Inline policies tend to drift — each owner edits their own copy in isolation, "
            "review tooling has nothing stable to diff against, and a permission widened on "
            "one role is invisible until somebody runs an access audit. The result is a slow "
            "creep toward over-permissive access."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC8.1"],
            "iso27001": ["A.5.15", "A.8.9", "A.8.32"],
            "hipaa": ["164.308(a)(3)", "164.308(a)(4)"],
            "nist": ["AC-2", "AC-6", "CM-2"],
            "pcidss": ["REQ-7", "REQ-2"],
        },
    },
    "iam-credential-rotation": {
        "description": (
            "{label}. Long-lived access keys and passwords accumulate exposure: every backup "
            "tape, every old git commit, every ex-employee laptop is another place the "
            "credential might be reachable. AWS guidance is to rotate every 90 days and to "
            "delete keys that have been unused for a comparable window."
        ),
        "risk": (
            "Static long-lived credentials are the bulk of the cloud-account-compromise "
            "fixture — leaked S3 buckets, GitHub commits, container images. The longer a key "
            "lives, the more accidental copies of it exist. Multiple keys per user multiplies "
            "the attack surface for no operational gain."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.3"],
            "iso27001": ["A.5.16", "A.5.17", "A.8.5"],
            "hipaa": ["164.308(a)(4)", "164.308(a)(5)(ii)(D)"],
            "nist": ["AC-2", "IA-5"],
            "pcidss": ["REQ-8.2", "REQ-8.3"],
        },
    },
    "iam-hygiene": {
        "description": (
            "{label}. The flagged IAM object isn't an immediate exposure but it muddies the "
            "access surface — unused groups, support-role gaps, dangling roles attached to no "
            "instance — and access reviews work better when the inventory matches reality."
        ),
        "risk": (
            "Stale IAM clutter masks real findings. Reviewers stop trusting the access list "
            "when it's clearly out of date, and an over-permissioned role tucked among twenty "
            "abandoned ones is much harder to spot. The fix is small and the audit-hygiene "
            "payoff is meaningful."
        ),
        "frameworks": {
            "soc2": ["CC6.3", "CC8.1"],
            "iso27001": ["A.5.15", "A.5.18", "A.8.9"],
            "hipaa": ["164.308(a)(3)", "164.308(a)(4)"],
            "nist": ["AC-2", "CM-2"],
            "pcidss": ["REQ-7"],
        },
    },
    "encryption-at-rest": {
        "description": (
            "{label}. AWS provides encryption-at-rest as a first-class default for every "
            "storage service involved here (S3, EBS, RDS, Redshift, DynamoDB, SQS, …) via "
            "either SSE-S3, SSE-KMS, or a customer-managed CMK. The flagged resource is "
            "storing data unencrypted at the storage layer."
        ),
        "risk": (
            "Unencrypted storage means a snapshot leak, a disk-level forensic recovery, or a "
            "misrouted backup yields data in plaintext. KMS-based encryption also lets the "
            "key policy revoke access en-masse during incident response — without it, key "
            "rotation is meaningless."
        ),
        "frameworks": {
            "soc2": ["CC6.7", "C1.1"],
            "iso27001": ["A.8.24", "A.5.23"],
            "hipaa": ["164.312(a)(2)(iv)", "164.312(c)(1)"],
            "nist": ["SC-13", "SC-28", "SC-12"],
            "pcidss": ["REQ-3", "REQ-3.5"],
        },
    },
    "tls-config": {
        "description": (
            "{label}. The TLS configuration on the flagged endpoint accepts protocol versions "
            "or cipher suites AWS has deprecated, or relies on a certificate the browser/AWS "
            "trust store no longer accepts. AWS publishes managed security policies that "
            "track current guidance — using one is the supported path."
        ),
        "risk": (
            "Old TLS versions (1.0/1.1) and weak ciphers are vulnerable to downgrade and "
            "padding-oracle attacks that retrieve plaintext from intercepted traffic. "
            "Deprecated certificates fail validation as clients update, eventually causing a "
            "production outage on top of the security gap."
        ),
        "frameworks": {
            "soc2": ["CC6.7", "CC6.8"],
            "iso27001": ["A.8.24", "A.8.20", "A.8.21"],
            "hipaa": ["164.312(e)(1)", "164.312(e)(2)(ii)"],
            "nist": ["SC-8", "SC-13", "SC-23"],
            "pcidss": ["REQ-4"],
        },
    },
    "cleartext-transit": {
        "description": (
            "{label}. The flagged endpoint accepts unencrypted HTTP / cleartext traffic — "
            "either as the only protocol, or alongside HTTPS without a redirect. AWS guidance "
            "is to terminate TLS at the edge and force every connection through it."
        ),
        "risk": (
            "Cleartext traffic means anyone on the network path — a misconfigured wifi "
            "captive portal, a hostile cafe, a downstream router — can read or modify the "
            "payload. Even endpoints that only carry public data leak credentials in cookies, "
            "auth headers, and query strings."
        ),
        "frameworks": {
            "soc2": ["CC6.7"],
            "iso27001": ["A.8.24", "A.8.20"],
            "hipaa": ["164.312(e)(1)", "164.312(e)(2)(ii)"],
            "nist": ["SC-8", "SC-13"],
            "pcidss": ["REQ-4"],
        },
    },
    "public-access": {
        "description": (
            "{label}. The flagged resource is reachable from outside the AWS account or "
            "exposes data to anonymous principals (`Principal: \"*\"`, a public ACL, a "
            "publicly routable IP, or a public snapshot). AWS guidance is to confine the "
            "resource to a private subnet, a specific account, or a named principal list."
        ),
        "risk": (
            "Publicly exposed AWS resources are continuously scanned by attackers — Shodan, "
            "censys, and dedicated bucket-enumeration tools find them within minutes of "
            "exposure. The most common AWS data breaches start as misconfigured public "
            "buckets or snapshots."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.6", "C1.1"],
            "iso27001": ["A.5.15", "A.8.20", "A.8.22"],
            "hipaa": ["164.308(a)(4)", "164.312(a)(1)", "164.312(e)(1)"],
            "nist": ["AC-3", "AC-6", "SC-7"],
            "pcidss": ["REQ-1", "REQ-7"],
        },
    },
    "security-group": {
        "description": (
            "{label}. The security group rule is wider than the workload behind it needs — "
            "a `0.0.0.0/0` ingress on an administrative port, a default SG carrying real "
            "rules, or a long-unused SG kept around as dead config. AWS guidance is to scope "
            "every rule to specific CIDRs, principals, or SG references."
        ),
        "risk": (
            "Wide-open security group rules give attackers direct network reach to backend "
            "services that were never hardened for the public internet — SSH brute force, "
            "RDP attacks, exposed database ports, unauthenticated metadata endpoints. The "
            "exposure persists 24/7."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC6.6"],
            "iso27001": ["A.8.20", "A.8.22", "A.5.15"],
            "hipaa": ["164.308(a)(4)", "164.312(a)(1)", "164.312(e)(1)"],
            "nist": ["AC-3", "SC-7", "CM-7"],
            "pcidss": ["REQ-1", "REQ-7"],
        },
    },
    "logging-disabled": {
        "description": (
            "{label}. AWS audit and access-log streams (CloudTrail, S3 access logs, ELB "
            "access logs, VPC flow logs, Config) are off or missing on the flagged surface. "
            "AWS guidance is to centralize these into a dedicated security-account S3 bucket "
            "with KMS encryption and integrity validation."
        ),
        "risk": (
            "Without complete logs, every post-incident investigation hits a wall — there is "
            "no record of who did what, when, or from where. Compliance frameworks treat "
            "missing audit trail as a high-severity finding because it forecloses detection "
            "AND response."
        ),
        "frameworks": {
            "soc2": ["CC7.2", "CC7.3"],
            "iso27001": ["A.8.15", "A.8.16"],
            "hipaa": ["164.308(a)(1)", "164.312(b)"],
            "nist": ["AU-2", "AU-3", "AU-12"],
            "pcidss": ["REQ-10", "REQ-10.2"],
        },
    },
    "cloudtrail-config": {
        "description": (
            "{label}. CloudTrail's configuration is the trail audit infrastructure depends on "
            "— log-file integrity validation, CloudWatch integration, KMS encryption, and "
            "global-services capture all need to be enabled deliberately for the trail to be "
            "credible during an investigation."
        ),
        "risk": (
            "A misconfigured CloudTrail is worse than no trail at all — defenders rely on it "
            "during incident response, and partial-data / no-integrity / no-encryption gaps "
            "create blind spots an attacker can exploit (and, with KMS-decrypt access, "
            "potentially tamper with after the fact)."
        ),
        "frameworks": {
            "soc2": ["CC7.2", "CC7.3", "CC7.4"],
            "iso27001": ["A.8.15", "A.8.16"],
            "hipaa": ["164.312(b)", "164.312(c)(1)"],
            "nist": ["AU-2", "AU-6", "AU-9", "AU-12"],
            "pcidss": ["REQ-10"],
        },
    },
    "monitoring-alarm": {
        "description": (
            "{label}. CIS-recommended CloudWatch alarms cover the security-relevant API "
            "surface — IAM changes, sign-in failures, root usage, NACL/SG/route-table edits, "
            "CMK deletion. Without alarms wired to a real notification channel the audit log "
            "is data nobody reads."
        ),
        "risk": (
            "Detection coverage is the gap between an attacker's first move and the "
            "responder's first action. Missing alarms on the API calls that signal "
            "compromise (CreateUser, DeleteTrail, root-account sign-in, unauthorized API "
            "calls) extend the window where an intrusion goes unanswered."
        ),
        "frameworks": {
            "soc2": ["CC7.2", "CC7.3"],
            "iso27001": ["A.8.15", "A.8.16"],
            "hipaa": ["164.308(a)(5)(ii)(C)", "164.308(a)(6)", "164.312(b)"],
            "nist": ["AU-6", "SI-4"],
            "pcidss": ["REQ-10", "REQ-11"],
        },
    },
    "backup-recovery": {
        "description": (
            "{label}. AWS-managed data stores ship automatic backup and recovery primitives "
            "(snapshots, PITR, cross-AZ replicas, versioning, deletion protection) but they "
            "have to be enabled per-resource. Default-off behaviors leave the resource one "
            "operator-error or one ransomware event away from data loss."
        ),
        "risk": (
            "A resource without backups, retention, or deletion protection is a single "
            "incident from permanent loss — and the incident isn't usually malicious. "
            "Accidental DROP TABLE, accidental terraform destroy, accidental S3 object "
            "overwrite all produce the same blast radius."
        ),
        "frameworks": {
            "soc2": ["A1.2", "CC7.4"],
            "iso27001": ["A.5.30", "A.8.13"],
            "hipaa": ["164.308(a)(7)", "164.312(c)(1)"],
            "nist": ["CP-9", "CP-10"],
            "pcidss": ["REQ-12"],
        },
    },
    "patch-update": {
        "description": (
            "{label}. The flagged AWS-managed component is on a deprecated version — an "
            "engine release with known CVEs, a certificate authority the trust store is "
            "phasing out, or an auto-upgrade path the workload has opted out of. The "
            "supported path is to apply the upgrade in a maintenance window."
        ),
        "risk": (
            "Software running on outdated AWS-managed engines is exposed to whatever CVEs the "
            "upgrade fixes — typically authentication bypasses, privilege escalation in stored "
            "procedures, or TLS-layer vulnerabilities. The longer the lag, the longer the "
            "window of exposure and the more disruptive the eventual catch-up upgrade."
        ),
        "frameworks": {
            "soc2": ["CC7.1", "CC8.1"],
            "iso27001": ["A.8.8", "A.8.9", "A.8.32"],
            "hipaa": ["164.308(a)(1)", "164.308(a)(6)"],
            "nist": ["CM-6", "SI-2"],
            "pcidss": ["REQ-6", "REQ-6.3"],
        },
    },
    "domain-management": {
        "description": (
            "{label}. Route 53 domain registration carries operational controls that protect "
            "against accidental loss of the registration — auto-renew, transfer lock, and "
            "registrar-side transfer authorization. Lapses cost the entire DNS hierarchy "
            "rooted at the domain."
        ),
        "risk": (
            "Loss of a registered domain is unique among AWS findings in that it can take "
            "down email, web, SSO, and customer-facing services simultaneously, often with "
            "limited recovery options. Domain-takeover incidents are rare but functionally "
            "catastrophic when they occur."
        ),
        "frameworks": {
            "soc2": ["A1.2", "CC8.1"],
            "iso27001": ["A.5.30", "A.8.9"],
            "hipaa": ["164.308(a)(7)"],
            "nist": ["CP-9", "CP-10"],
            "pcidss": ["REQ-12"],
        },
    },
    "network-topology": {
        "description": (
            "{label}. The VPC / networking surface flagged here is wider or noisier than the "
            "workload requires: public IPs on resources that don't need them, VPC peering "
            "without scoped routes, subnets without flow logs to inspect traffic. AWS "
            "guidance is to default to private subnets and add visibility (flow logs, GWLB) "
            "before adding reachability."
        ),
        "risk": (
            "Network exposure compounds — a public IP plus a wide security group plus an "
            "unpatched service is the standard chain leading to AWS instance compromise. "
            "Missing flow logs then deprive responders of the data needed to bound the "
            "blast radius after the fact."
        ),
        "frameworks": {
            "soc2": ["CC6.6", "CC7.2"],
            "iso27001": ["A.8.20", "A.8.21", "A.8.22"],
            "hipaa": ["164.308(a)(4)", "164.312(a)(1)", "164.312(e)(1)"],
            "nist": ["SC-7", "AU-2", "AU-12"],
            "pcidss": ["REQ-1", "REQ-10"],
        },
    },
    "data-exposure": {
        "description": (
            "{label}. The flagged surface puts data in places it shouldn't appear — "
            "credentials in EC2 user-data, secrets in container env-vars, MFA-Delete off on "
            "an S3 bucket holding durable state. AWS guidance is to keep secrets in Secrets "
            "Manager / Parameter Store and to lock buckets with MFA-Delete + versioning."
        ),
        "risk": (
            "Misplaced secrets are extracted by routine internal access — any read on the "
            "EC2 metadata service or the bucket reveals the secret. Versioning + MFA-Delete "
            "protects against the worst case where an attacker tries to cover their tracks "
            "by overwriting evidence."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "C1.1"],
            "iso27001": ["A.5.17", "A.8.3", "A.8.24"],
            "hipaa": ["164.308(a)(4)", "164.312(c)(1)"],
            "nist": ["AC-3", "SC-12", "SC-28"],
            "pcidss": ["REQ-3", "REQ-8.2"],
        },
    },
    "email-auth": {
        "description": (
            "{label}. SES domain identity DKIM signing authenticates outbound mail to "
            "downstream receivers; without it, mail is easier to spoof and gets penalized "
            "on reputation. AWS guidance is to enable DKIM on every verified sending "
            "identity."
        ),
        "risk": (
            "Missing DKIM lets attackers send convincingly-spoofed mail from the domain — "
            "phishing campaigns leveraging executive identities are an established initial-"
            "access pattern. Receivers also penalize unauthenticated mail, so legitimate "
            "transactional email starts landing in spam."
        ),
        "frameworks": {
            "soc2": ["CC6.6", "CC6.7"],
            "iso27001": ["A.8.21", "A.8.23"],
            "hipaa": ["164.312(e)(1)"],
            "nist": ["SC-8", "SC-23"],
            "pcidss": ["REQ-4", "REQ-5"],
        },
    },
    "infrastructure-hygiene": {
        "description": (
            "{label}. The flagged resource isn't an immediate security exposure but it "
            "carries operational debt — unused capacity, unowned configuration, mismatched "
            "naming. AWS guidance is to keep the inventory tight so reviews remain "
            "trustworthy."
        ),
        "risk": (
            "Operational debt accumulates until reviewers stop trusting the inventory and "
            "real findings get lost in the noise. The fix is incremental and low-risk; the "
            "long-run payoff is a cleaner attack surface and faster reviews."
        ),
        "frameworks": {
            "soc2": ["CC6.3", "CC8.1"],
            "iso27001": ["A.5.18", "A.8.9", "A.8.32"],
            "hipaa": ["164.308(a)(1)", "164.308(a)(3)"],
            "nist": ["AC-2", "CM-2", "CM-6"],
            "pcidss": ["REQ-2", "REQ-7"],
        },
    },
    # Generic catch-all — used only when no more-specific pattern matches.
    "general": {
        "description": (
            "{label}. The flagged configuration deviates from AWS-published security "
            "guidance for the affected service. The remediation section below describes the "
            "supported way to bring the resource back into a compliant state."
        ),
        "risk": (
            "Configuration drift compounds — each individual deviation is small, but the "
            "aggregate is what determines whether an incident stays contained or escalates. "
            "Address the finding to keep the baseline tight."
        ),
        "frameworks": {
            "soc2": ["CC6.1", "CC8.1"],
            "iso27001": ["A.5.23", "A.8.9"],
            "hipaa": ["164.308(a)(1)"],
            "nist": ["CM-6"],
            "pcidss": ["REQ-2"],
        },
    },
}


def _has(rk: str, *needles: str) -> bool:
    return all(n in rk for n in needles)


def _any(rk: str, *needles: str) -> bool:
    return any(n in rk for n in needles)


def categorize(rule_key: str) -> str:
    """Pick the most specific category for a rule_key. Order matters —
    more-specific patterns first."""
    rk = rule_key
    if rk.startswith("iam-password-policy-"):
        return "password-policy"
    if _any(rk, "no-mfa", "without-mfa", "lacks-external-id-and-mfa", "no-hardware-mfa"):
        return "mfa"
    if rk.startswith("iam-root-account"):
        return "root-account"
    if rk.startswith("iam-") and (
        "allows-non-sts-action" in rk
        or "allows-NotActions" in rk
        or "allows-full-privileges" in rk
        or "policy-allows-all" in rk
        or "lightspin-user-action-denied-for-group" in rk
    ):
        return "iam-privilege"
    if "inline-policy" in rk or "with-inline-policies" in rk:
        return "iam-inline-policy"
    if rk in {
        "iam-user-no-key-rotation",
        "iam-unused-credentials-not-disabled",
        "iam-user-unused-access-key-initial-setup",
        "iam-user-with-multiple-access-keys",
        "iam-user-with-password-and-key",
    }:
        return "iam-credential-rotation"
    if rk.startswith("iam-"):
        return "iam-hygiene"
    if _any(
        rk,
        "not-encrypted",
        "no-encryption",
        "encryption-disabled",
        "no-default-encryption",
        "default-encryption-disabled",
        "cmk-rotation-disabled",
        "server-side-encryption-disabled",
    ):
        return "encryption-at-rest"
    if _any(
        rk,
        "older-ssl-policy",
        "ssl-not-required",
        "ca-certificate-deprecated",
        "insufficient-viewer-security",
        "with-invalid-certificate",
        "http-request-smuggling",
    ):
        return "tls-config"
    if _any(
        rk,
        "cleartext",
        "insecure-origin",
        "no-https",
    ):
        return "cleartext-transit"
    if _any(
        rk,
        "publicly-accessible",
        "with-public-ip",
        "world-policy-star",
    ) or rk in {
        "ec2-ami-public",
        "ec2-ebs-snapshot-public",
        "rds-snapshot-public",
    }:
        return "public-access"
    if "security-group" in rk:
        return "security-group"
    if rk.startswith("logs-no-alarm-") or rk == "cloudwatch-alarm-without-actions":
        return "monitoring-alarm"
    if rk.startswith("cloudtrail-"):
        if rk in {"cloudtrail-no-logging", "cloudtrail-not-configured", "cloudtrail-no-data-logging", "cloudtrail-partial-data-logging", "cloudtrail-no-global-services-logging", "cloudtrail-duplicated-global-services-logging"}:
            return "logging-disabled"
        return "cloudtrail-config"
    if rk == "config-recorder-not-configured":
        return "logging-disabled"
    if _any(
        rk,
        "no-logs",
        "no-logging",
        "no-flow-log",
        "no-access-logs",
        "parameter-group-logging-disabled",
        "without-flow-log",
    ):
        return "logging-disabled"
    if "transparency-logging-disabled" in rk:
        # ACM cert transparency: still a TLS hygiene concern.
        return "tls-config"
    if rk in {
        "rds-instance-backup-disabled",
        "rds-instance-short-backup-retention-period",
        "rds-instance-single-az",
        "s3-bucket-no-versioning",
        "s3-bucket-no-mfa-delete",
        "elbv2-no-deletion-protection",
    }:
        return "backup-recovery"
    if _any(rk, "no-version-upgrade", "no-minor-upgrade"):
        return "patch-update"
    if rk.startswith("route53-domain-"):
        return "domain-management"
    if rk.startswith("vpc-") or rk == "ec2-instance-with-public-ip":
        return "network-topology"
    if rk in {
        "ec2-instance-with-user-data-secrets",
    }:
        return "data-exposure"
    if rk.startswith("ses-identity-dkim"):
        return "email-auth"
    if rk in {
        "cloudformation-stack-with-role",
        "ec2-instance-type",
        "ec2-instance-types",
        "ec2-default-security-group-in-use",
    }:
        return "infrastructure-hygiene"
    return "general"


# ----------------------------------------------------------------------
# Per-rule label generation. Each rule_key turns into a short
# sentence-case display name used in the description / risk templates.
# ----------------------------------------------------------------------

LABEL_OVERRIDES: dict[str, str] = {
    "acm-certificate-with-transparency-logging-disabled": (
        "the ACM certificate has Certificate Transparency logging disabled"
    ),
    "cloudformation-stack-with-role": (
        "the CloudFormation stack runs with an assumed IAM role"
    ),
    "cloudfront-distribution-cleartext-origin": (
        "the CloudFront distribution fetches from its origin over cleartext HTTP"
    ),
    "cloudfront-distribution-insecure-origin": (
        "the CloudFront distribution accepts insecure origin protocols"
    ),
    "cloudfront-distribution-insufficient-viewer-security": (
        "the CloudFront viewer security policy allows weak TLS configurations"
    ),
    "cloudtrail-duplicated-global-services-logging": (
        "global-services CloudTrail events are logged by multiple trails (duplicate billing + log noise)"
    ),
    "cloudtrail-no-cloudwatch-integration": (
        "CloudTrail is not delivering events to CloudWatch Logs"
    ),
    "cloudtrail-no-data-logging": (
        "CloudTrail is not capturing data-event activity (S3 / Lambda / DynamoDB)"
    ),
    "cloudtrail-no-encryption-with-kms": (
        "CloudTrail log files are not KMS-encrypted at rest"
    ),
    "cloudtrail-no-global-services-logging": (
        "CloudTrail is not capturing global-service events (IAM, CloudFront, Route 53)"
    ),
    "cloudtrail-no-log-file-validation": (
        "CloudTrail log-file integrity validation is disabled"
    ),
    "cloudtrail-no-logging": (
        "the CloudTrail trail is not actively logging"
    ),
    "cloudtrail-not-configured": (
        "CloudTrail is not configured in this account / region"
    ),
    "cloudtrail-partial-data-logging": (
        "CloudTrail data-event logging covers only a subset of relevant resources"
    ),
    "cloudwatch-alarm-without-actions": (
        "a CloudWatch alarm has no alarm actions configured"
    ),
    "config-recorder-not-configured": (
        "AWS Config recorder is not running in this account / region"
    ),
    "ec2-ami-public": (
        "an EC2 AMI is shared publicly"
    ),
    "ec2-default-security-group-in-use": (
        "the default VPC security group is attached to a workload"
    ),
    "ec2-default-security-group-with-rules": (
        "the default VPC security group carries custom rules"
    ),
    "ec2-ebs-default-encryption-disabled": (
        "EBS default encryption is disabled in this region"
    ),
    "ec2-ebs-snapshot-not-encrypted": (
        "an EBS snapshot is unencrypted"
    ),
    "ec2-ebs-snapshot-public": (
        "an EBS snapshot is shared publicly"
    ),
    "ec2-ebs-volume-not-encrypted": (
        "an EBS volume is unencrypted"
    ),
    "ec2-instance-type": (
        "an EC2 instance uses an instance type outside the approved list"
    ),
    "ec2-instance-types": (
        "EC2 instance types in use deviate from the approved baseline"
    ),
    "ec2-instance-with-public-ip": (
        "an EC2 instance has a routable public IP"
    ),
    "ec2-instance-with-user-data-secrets": (
        "the EC2 instance user-data script appears to carry credentials"
    ),
    "ec2-security-group-opens-known-port-to-all": (
        "a security group opens a well-known port to the public internet"
    ),
    "ec2-unused-security-group": (
        "an EC2 security group is defined but attached to no resource"
    ),
    "elb-listener-allowing-cleartext": (
        "an ELB listener accepts cleartext HTTP traffic"
    ),
    "elb-no-access-logs": (
        "the classic ELB has access logging disabled"
    ),
    "elb-older-ssl-policy": (
        "the classic ELB uses a deprecated SSL/TLS policy"
    ),
    "elbv2-http-request-smuggling": (
        "an ALB / NLB target group is exposed to HTTP request smuggling"
    ),
    "elbv2-listener-allowing-cleartext": (
        "an ALB / NLB listener accepts cleartext HTTP traffic"
    ),
    "elbv2-no-access-logs": (
        "the ALB / NLB has access logging disabled"
    ),
    "elbv2-no-deletion-protection": (
        "the ALB / NLB has deletion protection disabled"
    ),
    "elbv2-older-ssl-policy": (
        "the ALB / NLB uses a deprecated TLS policy"
    ),
    "iam-assume-role-lacks-external-id-and-mfa": (
        "an IAM role's trust policy allows cross-account assumption without an ExternalId or MFA"
    ),
    "iam-assume-role-no-mfa": (
        "an IAM role's trust policy allows assumption without an MFA condition"
    ),
    "iam-assume-role-policy-allows-all": (
        "an IAM role's trust policy allows assumption from any principal"
    ),
    "iam-ec2-role-without-instances": (
        "an IAM role intended for EC2 use is attached to no instances"
    ),
    "iam-group-with-inline-policies": (
        "an IAM group carries inline policies instead of attached managed policies"
    ),
    "iam-group-with-no-users": (
        "an IAM group has no users assigned"
    ),
    "iam-inline-policy-allows-NotActions": (
        "an inline policy uses NotAction to define an allow list (over-broad by construction)"
    ),
    "iam-inline-policy-allows-non-sts-action": (
        "an inline policy on an STS-only context grants non-STS actions"
    ),
    "iam-inline-policy-for-role": (
        "an IAM role carries an inline policy rather than a managed attachment"
    ),
    "iam-lightspin-user-action-denied-for-group": (
        "an IAM identity has actions denied for a group it belongs to (privilege ambiguity)"
    ),
    "iam-managed-policy-allows-NotActions": (
        "a managed policy uses NotAction to define an allow list (over-broad by construction)"
    ),
    "iam-managed-policy-allows-full-privileges": (
        "a managed policy grants full administrative privileges"
    ),
    "iam-managed-policy-allows-non-sts-action": (
        "a managed policy on an STS-only context grants non-STS actions"
    ),
    "iam-managed-policy-for-role": (
        "an IAM role attaches a customer-managed policy in a context that should be AWS-managed"
    ),
    "iam-managed-policy-no-attachments": (
        "a managed policy is defined but attached to no principal"
    ),
    "iam-no-support-role": (
        "no IAM role is mapped to the AWSSupportAccess managed policy"
    ),
    "iam-password-policy-expiration-threshold": (
        "a password expiration period above the recommended threshold"
    ),
    "iam-password-policy-minimum-length": (
        "the minimum-length requirement for console passwords"
    ),
    "iam-password-policy-no-expiration": (
        "console password expiration"
    ),
    "iam-password-policy-no-lowercase-required": (
        "the lowercase-character requirement for console passwords"
    ),
    "iam-password-policy-no-number-required": (
        "the digit requirement for console passwords"
    ),
    "iam-password-policy-no-symbol-required": (
        "the symbol-character requirement for console passwords"
    ),
    "iam-password-policy-no-uppercase-required": (
        "the uppercase-character requirement for console passwords"
    ),
    "iam-password-policy-reuse-enabled": (
        "a password-reuse prevention window"
    ),
    "iam-role-with-inline-policies": (
        "an IAM role carries inline policies instead of attached managed policies"
    ),
    "iam-root-account-no-hardware-mfa": (
        "the root account has no hardware MFA device enrolled"
    ),
    "iam-root-account-no-mfa": (
        "the root account has no MFA device enrolled"
    ),
    "iam-root-account-used-recently": (
        "the root account was used recently"
    ),
    "iam-root-account-with-active-certs": (
        "the root account has active signing certificates"
    ),
    "iam-root-account-with-active-keys": (
        "the root account has active programmatic access keys"
    ),
    "iam-unused-credentials-not-disabled": (
        "IAM credentials that have been unused for a long window remain active"
    ),
    "iam-user-no-key-rotation": (
        "an IAM user access key has not been rotated within the recommended window"
    ),
    "iam-user-unused-access-key-initial-setup": (
        "an IAM user's initial access key has never been used"
    ),
    "iam-user-with-multiple-access-keys": (
        "an IAM user has multiple active access keys"
    ),
    "iam-user-with-password-and-key": (
        "an IAM user has both a console password and an active programmatic key"
    ),
    "iam-user-with-policies": (
        "an IAM user has policies attached directly rather than via group membership"
    ),
    "iam-user-without-mfa": (
        "an IAM user with console access has no MFA device enrolled"
    ),
    "kms-cmk-rotation-disabled": (
        "a customer-managed KMS CMK has automatic rotation disabled"
    ),
    "logs-no-alarm-aws-configuration-changes": (
        "no CloudWatch alarm fires on AWS Config changes"
    ),
    "logs-no-alarm-cloudtrail-configuration-changes": (
        "no CloudWatch alarm fires on CloudTrail configuration changes"
    ),
    "logs-no-alarm-cmk-deletion": (
        "no CloudWatch alarm fires on customer-managed CMK deletion"
    ),
    "logs-no-alarm-console-authentication-failures": (
        "no CloudWatch alarm fires on console authentication failures"
    ),
    "logs-no-alarm-iam-policy-changes": (
        "no CloudWatch alarm fires on IAM policy changes"
    ),
    "logs-no-alarm-nacl-changes": (
        "no CloudWatch alarm fires on network ACL changes"
    ),
    "logs-no-alarm-network-gateways-changes": (
        "no CloudWatch alarm fires on network gateway changes"
    ),
    "logs-no-alarm-root-usage": (
        "no CloudWatch alarm fires on root-account usage"
    ),
    "logs-no-alarm-route-table-changes": (
        "no CloudWatch alarm fires on route-table changes"
    ),
    "logs-no-alarm-s3-policy-changes": (
        "no CloudWatch alarm fires on S3 bucket policy changes"
    ),
    "logs-no-alarm-security-group-changes": (
        "no CloudWatch alarm fires on security-group changes"
    ),
    "logs-no-alarm-signin-without-mfa": (
        "no CloudWatch alarm fires on console sign-in without MFA"
    ),
    "logs-no-alarm-unauthorized-api-calls": (
        "no CloudWatch alarm fires on unauthorized API calls"
    ),
    "logs-no-alarm-vpc-changes": (
        "no CloudWatch alarm fires on VPC configuration changes"
    ),
    "rds-instance-backup-disabled": (
        "the RDS instance has automated backups disabled"
    ),
    "rds-instance-ca-certificate-deprecated": (
        "the RDS instance uses a deprecated certificate authority"
    ),
    "rds-instance-no-minor-upgrade": (
        "the RDS instance has auto minor-version upgrades disabled"
    ),
    "rds-instance-publicly-accessible": (
        "the RDS instance is publicly accessible"
    ),
    "rds-instance-short-backup-retention-period": (
        "the RDS automated-backup retention window is shorter than recommended"
    ),
    "rds-instance-single-az": (
        "the RDS instance is single-AZ (no multi-AZ failover)"
    ),
    "rds-instance-storage-not-encrypted": (
        "the RDS instance storage is unencrypted at rest"
    ),
    "rds-postgres-instance-with-invalid-certificate": (
        "an RDS PostgreSQL client is configured with an invalid server certificate"
    ),
    "rds-snapshot-public": (
        "an RDS snapshot is shared publicly"
    ),
    "redshift-cluster-database-not-encrypted": (
        "a Redshift cluster's database storage is unencrypted at rest"
    ),
    "redshift-cluster-no-version-upgrade": (
        "a Redshift cluster has auto version upgrades disabled"
    ),
    "redshift-cluster-publicly-accessible": (
        "a Redshift cluster is publicly accessible"
    ),
    "redshift-parameter-group-logging-disabled": (
        "a Redshift parameter group has audit logging disabled"
    ),
    "redshift-parameter-group-ssl-not-required": (
        "a Redshift parameter group does not require SSL connections"
    ),
    "route53-domain-no-autorenew": (
        "a Route 53 registered domain has auto-renew disabled"
    ),
    "route53-domain-no-transferlock": (
        "a Route 53 registered domain has transfer-lock disabled"
    ),
    "route53-domain-transferlock-not-authorized": (
        "the Route 53 transfer-lock request is not authorized"
    ),
    "s3-bucket-allowing-cleartext": (
        "an S3 bucket policy allows cleartext HTTP requests"
    ),
    "s3-bucket-no-default-encryption": (
        "an S3 bucket has no default encryption configured"
    ),
    "s3-bucket-no-logging": (
        "an S3 bucket has access logging disabled"
    ),
    "s3-bucket-no-mfa-delete": (
        "an S3 bucket has MFA Delete disabled"
    ),
    "s3-bucket-no-versioning": (
        "an S3 bucket has versioning disabled"
    ),
    "s3-bucket-world-policy-star": (
        "an S3 bucket policy uses Principal: \"*\" (publicly accessible)"
    ),
    "ses-identity-dkim-not-enabled": (
        "an SES sending identity has DKIM signing disabled"
    ),
    "ses-identity-dkim-not-verified": (
        "an SES sending identity's DKIM configuration is unverified"
    ),
    "sqs-queue-server-side-encryption-disabled": (
        "an SQS queue has server-side encryption disabled"
    ),
    "vpc-routing-tables-with-peering": (
        "a VPC route table mixes peering routes with non-peering destinations"
    ),
    "vpc-subnet-without-flow-log": (
        "a VPC subnet has no VPC Flow Logs configured"
    ),
}


def label_for(rule_key: str) -> str:
    return LABEL_OVERRIDES.get(rule_key, rule_key.replace("-", " "))


def display_name_for(rule_key: str) -> str:
    """Short title-cased display label used as the article 'name'."""
    return rule_key.replace("-", " ").title()


# ----------------------------------------------------------------------
# Driver
# ----------------------------------------------------------------------


def main() -> int:
    if not METADATA_PATH.exists():
        print(f"metadata not found at {METADATA_PATH}", file=sys.stderr)
        return 1
    if not MAPPINGS_PATH.exists():
        print(f"mappings not found at {MAPPINGS_PATH}", file=sys.stderr)
        return 1

    with open(METADATA_PATH, "r", encoding="utf-8") as f:
        metadata = json.load(f)
    with open(MAPPINGS_PATH, "r", encoding="utf-8") as f:
        mappings = json.load(f)

    # 1) Extend frameworks registry with PCI-DSS.
    mappings["frameworks"]["pcidss"] = {"name": "PCI DSS v4.0"}

    # 2) For every rule_key in metadata, fill in description / risk / name
    #    (preserving any existing fields) and ensure all five generated
    #    frameworks are present in mappings.
    rule_keys = sorted(metadata.keys())

    def cap_first(s: str) -> str:
        # Templates often start with `{label}` — when the label resolves
        # to something starting with a lowercase article ("an EBS
        # snapshot is unencrypted"), the rendered sentence reads as
        # broken prose. Force the first letter to uppercase so
        # "an EBS …" → "An EBS …" without touching the rest.
        if not s:
            return s
        return s[0].upper() + s[1:]

    generated_meta: "OrderedDict[str, dict]" = OrderedDict()
    for rk in rule_keys:
        category = categorize(rk)
        template = CATEGORIES[category]
        label = label_for(rk)
        description = cap_first(template["description"].format(label=label))
        risk = cap_first(template["risk"].format(label=label))

        upstream = metadata[rk]
        entry = OrderedDict()
        entry["name"] = display_name_for(rk)
        entry["category"] = category
        entry["description"] = description
        entry["risk"] = risk
        # Pass-through upstream fields unchanged.
        if "remediation" in upstream and upstream["remediation"]:
            entry["remediation"] = upstream["remediation"]
        if "references" in upstream and upstream["references"]:
            entry["references"] = upstream["references"]
        if "compliance" in upstream and upstream["compliance"]:
            entry["compliance"] = upstream["compliance"]
        generated_meta[rk] = entry

        # Mappings: generated content fills any framework slot the hand-
        # authored mappings.json doesn't already cover. Hand-authored
        # entries always win on collision.
        per_finding = mappings["mappings"].setdefault(rk, {})
        for fw_id, control_keys in template["frameworks"].items():
            if fw_id in per_finding:
                # Hand-authored entry exists. Keep it untouched so a
                # carefully curated mapping doesn't get clobbered by a
                # one-size-fits-all template.
                continue
            fw_table = {
                "soc2": SOC2,
                "iso27001": ISO,
                "hipaa": HIPAA,
                "nist": NIST,
                "pcidss": PCI,
            }[fw_id]
            controls = []
            for key in control_keys:
                control_id, title = fw_table[key]
                controls.append({"control_id": control_id, "title": title})
            per_finding[fw_id] = controls

    # 3) Sort mappings keys for byte-stable output.
    mappings["mappings"] = OrderedDict(
        sorted(mappings["mappings"].items())
    )
    for k, v in mappings["mappings"].items():
        mappings["mappings"][k] = OrderedDict(sorted(v.items()))

    # 4) Write.
    with open(METADATA_PATH, "w", encoding="utf-8") as f:
        json.dump(generated_meta, f, indent=2, ensure_ascii=False)
        f.write("\n")
    with open(MAPPINGS_PATH, "w", encoding="utf-8") as f:
        json.dump(mappings, f, indent=2, ensure_ascii=False)
        f.write("\n")

    # 5) Summary.
    from collections import Counter
    cats = Counter(categorize(k) for k in rule_keys)
    print(f"generated content for {len(rule_keys)} rules")
    for cat, count in sorted(cats.items()):
        print(f"  {cat:30s}  {count:3d}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
