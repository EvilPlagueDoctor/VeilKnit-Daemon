# GUI v2 vs current mailbox/profile build

The older archive used for the comparison was `VeilKnit-Daemon-GUI-Apps-Approval-Language-v2.zip`.

## What the older GUI version contained

- Structured Applications page with observed-app counts.
- Application approval de-duplication and newest-request-wins behavior.
- Checked request list with Refresh Requests, Allow Checked, and Refuse Checked.
- Application-visible-name / alias management.
- Network profile management controls.
- Five-language Windows UI and Android language support.
- Structured network summary blocks.
- App Directory / app-focused discovery support.
- Android Backup page.
- Windows login-screen Restore backup button.

## What the current mailbox/profile build added

- Delegatable, deliberately-readable short-lived `ServiceRequest` mailbox records.
- Temporary reply-route handling and service-request subscriptions.
- Service-request SDK APIs for publish, withdraw, subscribe, and reply.
- VeilySocial Profile/Widget application replacing the business-card UI for the main sample.
- Profile-page blob publishing and hash verification.
- MinHash/gossip discovery backbone and progressive profile verification.
- Android and Windows profile designer/viewer integration.

## Important comparison result

All files from the older GUI archive were present in the newer profile/mailbox build. The Windows C++ GUI source and Android daemon GUI source were byte-identical before this merge; the newer package had additional backend/mailbox/profile files and changed mailbox/API files.

The remembered **Applications first / Backup second** post-login behavior was not present in the older archive itself. This merge adds that behavior explicitly.

Windows also now has a dedicated Backup tab matching the Android backup/recovery workflow. The existing pre-login Restore backup button remains on Overview.
