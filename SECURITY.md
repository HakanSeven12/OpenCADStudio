# Security Policy

The OpenCADStudio maintainers take security vulnerabilities seriously. We appreciate responsible reports that help protect users, their drawings, and their systems.

Please do not publicly disclose a suspected vulnerability before the maintainers have had a reasonable opportunity to investigate and release a fix.

## Supported Versions

OpenCADStudio is under active development. Security updates are generally provided for the latest released version and the current `main` branch.

| Version                       | Supported |
| ----------------------------- | --------- |
| Latest release                | Yes       |
| Current `main` branch         | Yes       |
| Older releases                | No        |
| Unofficial or modified builds | No        |

Users should reproduce security issues using the latest release or the latest version of the `main` branch whenever possible.

## Reporting a Vulnerability

### Preferred reporting method

Use GitHub Private Vulnerability Reporting when it is available:

1. Open the repository's **Security** tab.
2. Select **Advisories**.
3. Select **Report a vulnerability**.
4. Provide the information requested below.

Repository:

`https://github.com/HakanSeven12/OpenCADStudio`

### When private vulnerability reporting is unavailable

Do not publish technical vulnerability details, proof-of-concept code, malicious files, credentials, or exploit instructions in a public GitHub issue.

Instead, create a minimal public issue that only states that you need a private security contact. Wait for a maintainer to provide a private communication channel before sharing sensitive details.

A minimal issue may contain:

```text
I believe I have identified a potential security vulnerability in OpenCADStudio. Please provide a private communication channel so I can share the details responsibly.
```

## Information to Include

A useful security report should include:

* A clear description of the vulnerability
* The affected OpenCADStudio version or commit
* The affected operating system or browser
* Whether the issue affects the native application, web application, file parser, exporter, plug-in system, or build process
* Step-by-step reproduction instructions
* The expected and actual behavior
* The security impact
* Any required user interaction
* A minimal proof of concept, when safe to provide
* A sample DWG, DXF, STL, STEP, OBJ, PDF, plug-in, or configuration file when relevant
* Crash logs, stack traces, browser console output, or screenshots
* Suggested mitigations or fixes, when available
* Whether the vulnerability has been disclosed to anyone else

Remove personal information, proprietary drawings, customer data, access tokens, credentials, and unrelated confidential information from submitted files.

## Security Scope

Examples of vulnerabilities that are considered in scope include:

### File parsing and importing

* Arbitrary code execution caused by opening a crafted file
* Memory corruption
* Out-of-bounds access
* Integer overflow with a security impact
* Uncontrolled memory allocation or resource exhaustion
* Infinite loops triggered by small malicious files
* Path traversal during extraction, import, export, or save operations
* Unsafe handling of embedded previews, images, fonts, or references
* Unexpected access to local files through external references

### File writing and exporting

* Writing files outside the location selected by the user
* Overwriting unrelated files without warning
* Path traversal through drawing names, layouts, blocks, references, or export settings
* Generated files containing unintended local data
* Malicious output that causes a security issue in another supported application

### Native application

* Arbitrary command execution
* Unsafe loading of libraries or executables
* DLL search-order hijacking
* Privilege escalation
* Insecure temporary file handling
* Sensitive information exposure
* Security boundaries bypassed through command-line arguments or environment variables

### Web application and WebAssembly

* Cross-site scripting
* Arbitrary JavaScript execution
* Unauthorized access to local browser storage or selected files
* Unsafe use of browser file system APIs
* Origin or sandbox boundary bypasses
* Sensitive information stored or transmitted unexpectedly
* Denial of service caused by a small malicious drawing or input

### Plug-ins and add-ons

* Loading untrusted plug-ins without clear user consent
* Plug-ins escaping documented isolation boundaries
* Improper validation of plug-in manifests
* Unauthorized file system, network, or process access
* Privilege escalation between the host application and a plug-in
* Unsafe inter-process communication
* Security issues in the plug-in marketplace or update mechanism

### Dependencies and build process

* Compromised dependencies
* Dependency confusion
* Unsafe GitHub Actions workflows
* Exposure of repository secrets
* Release artifacts that do not match the published source
* Insecure update or distribution processes

## Out of Scope

The following are generally not considered security vulnerabilities:

* Normal application crashes without a meaningful security impact
* Performance problems caused only by extremely large or complex drawings
* Visual rendering defects
* Incorrect CAD geometry without a security impact
* Missing features
* Usability problems
* Social engineering
* Phishing
* Vulnerabilities that require a user to intentionally install and trust a clearly untrusted third-party plug-in
* Issues that only affect unsupported versions
* Vulnerabilities in third-party applications or operating systems
* Automated scanner reports without manual verification
* Dependency vulnerability reports without evidence that OpenCADStudio is affected
* Denial-of-service tests that require excessive traffic or infrastructure load
* Physical attacks on a user's device
* Reports based only on outdated dependency version numbers without a demonstrated exploit path

A crash may still be considered a security issue when it is caused by a small crafted file, results in memory corruption, exposes sensitive data, or creates a realistic denial-of-service risk.

## Testing Guidelines

Security research must be performed responsibly.

You may:

* Test systems, repositories, accounts, and files that you own
* Test the publicly available OpenCADStudio application
* Create malicious test drawings in a controlled environment
* Use local virtual machines or isolated test systems
* Perform limited automated testing that does not affect other users

You must not:

* Access data belonging to other users
* Test against systems without authorization
* Upload malicious files to public issues or discussions
* Disrupt the project website, GitHub repository, releases, or other infrastructure
* Perform denial-of-service attacks
* Use social engineering or phishing
* Distribute weaponized exploits
* Publish vulnerabilities before coordinated disclosure
* Retain, modify, or disclose data discovered unintentionally

Stop testing and report the issue immediately if you encounter credentials, private files, personal data, or data belonging to another person.

## Response Process

The maintainers will aim to:

* Acknowledge a report within 7 days
* Confirm whether the report is being investigated
* Request additional information when needed
* Provide a status update within 14 days
* Coordinate a fix and disclosure timeline when the vulnerability is confirmed

These are targets rather than guaranteed deadlines. Response time may depend on maintainer availability, issue complexity, and the severity of the vulnerability.

Duplicate reports may be closed or linked to an existing private report.

## Severity Assessment

Vulnerabilities may be evaluated using factors including:

* Required user interaction
* Attack complexity
* Whether a malicious drawing must be opened
* Whether the issue affects the web or native application
* Confidentiality impact
* Integrity impact
* Availability impact
* Ability to execute code
* Ability to read or modify arbitrary files
* Ability to escape plug-in or browser boundaries
* Number of affected users
* Availability of mitigations

The maintainers may use CVSS as supporting guidance, but the final severity assessment remains at their discretion.

## Coordinated Disclosure

Please allow the maintainers sufficient time to investigate and prepare a fix before public disclosure.

When a vulnerability is confirmed, the maintainers may:

* Prepare a private patch
* Request validation from the reporter
* Publish a fixed release
* Create a GitHub Security Advisory
* Request a CVE identifier
* Credit the reporter
* Publish technical details after users have had time to update

Reporter credit is optional. Tell the maintainers how you would like to be credited, or state that you prefer to remain anonymous.

## Security Updates

Security fixes may be delivered through:

* A new OpenCADStudio release
* A GitHub Security Advisory
* Release notes
* Repository documentation
* A commit to the `main` branch

Users should download releases only from the official repository:

`https://github.com/HakanSeven12/OpenCADStudio/releases`

Users are encouraged to verify that the repository owner and release source are correct before running downloaded binaries.

## Safe Harbor

The maintainers support good-faith security research performed in accordance with this policy.

The project will not intentionally pursue legal action against researchers who:

* Follow this policy
* Avoid privacy violations and service disruption
* Report vulnerabilities promptly
* Provide the maintainers reasonable time to respond
* Do not exploit vulnerabilities beyond what is necessary to demonstrate the issue
* Do not access, modify, retain, or disclose data belonging to others

This safe-harbor statement does not authorize testing of third-party systems and does not override applicable laws or agreements.

## Public Bug Reports

Regular bugs, crashes, feature requests, and usability issues that do not contain sensitive security information may be reported through GitHub Issues:

`https://github.com/HakanSeven12/OpenCADStudio/issues`
