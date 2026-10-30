# Sponsor and Partner Preset Policy (proposal)

> **Status: proposal for the maintainers.** Nothing here is adopted until a maintainer accepts it and removes this notice. It describes how sponsored providers could be listed, reviewed and removed, so that users and companies can see why a relay appears in CC Switch.

## Why this is needed

Sponsors fund the project, and the README and the provider picker reflect that:

- About half of the built-in Claude presets (35 of 70 in `src/config/claudeProviderPresets.ts`) and of the Codex presets (34 of 61) carry `isPartner: true`, and `primePartner` pins some of them to the top of the preset picker (`src/components/providers/forms/ProviderPresetSelector.tsx`).
- Sponsor descriptions in the README repeat the sponsors' own claims (for example "100% sourced from official channels" or compensation guarantees) that the project cannot check.
- Many sign-up links carry referral parameters (`aff=`, `ref=`, `invitecode=`, `utm_source=`), but nothing says so.
- Additions, demotions and removals appear only as single changelog lines (for example the 3.16.0 and 3.16.3 entries in `CHANGELOG.md`), with no stated reason or rule.
- A relay sees every prompt and every line of code sent through it (see [PRIVACY.md](../PRIVACY.md)), so a bad sponsor can harm users directly.

## 1. What sponsorship buys

Sponsorship buys **visibility only**:

- a listing in the README sponsor section, with logo and a short description;
- a preset in the provider picker marked as a partner, optionally pinned near the top;
- optional promotional text in the app (for example a discount code), shown as the sponsor's text.

Sponsorship never buys:

- being the default provider, or being selected, switched to or enabled automatically;
- any change to routing, failover order, model mapping, pricing data or usage accounting;
- removal or demotion of competitors, or changes to how non-sponsored presets are shown;
- access to any user data. CC Switch has no telemetry and does not report anything to sponsors.

## 2. Listing criteria

Before a sponsored preset or README entry is merged, the sponsor provides, and a maintainer records in the PR:

| Requirement | Evidence |
| --- | --- |
| A legal entity | Company name and country of registration |
| Terms of service and a privacy policy that cover prompt/data retention | Public URLs |
| A refund policy for prepaid balances | Public URL |
| A status page or other public incident channel | URL |
| An abuse and security contact | Email address that is answered |
| HTTPS endpoints only, on domains the sponsor controls | The preset's `baseURL` values |
| Honest model naming | Statement that models listed under a vendor's name (Claude, GPT, Gemini, ...) are served by that vendor's model, or a clear note where they are not |
| Accurate copy | README and in-app text limited to verifiable facts; performance or "no dilution" claims are either removed or labelled as the sponsor's own claim |

The maintainers may decline any sponsor without giving a reason.

## 3. Disclosure

- The README sponsor section starts with a sentence saying that these are paid placements and that some links are referral links that earn the project credit.
- Referral links are labelled (for example "referral link") wherever they appear in the README.
- In the app, partner presets keep their partner badge, and the preset tooltip or details say "Sponsored".
- Sponsor descriptions are written as the sponsor's statement ("X says ..."), not as the project's endorsement.

## 4. Review

- **Cadence**: every partner preset is re-checked at least every six months, and at each minor release where presets change.
- **Checks**: the criteria in section 2 still hold; endpoints respond; listed models still exist; no unresolved user reports of the issues in section 5.
- **Record**: each partner preset gets a "last reviewed" date. A simple option is a `reviewedAt` field next to `isPartner` in the preset files, or a table in this document.

## 5. Removal and demotion

A sponsor is **removed immediately** (patch release if needed) when there is credible evidence of:

- logging, reselling or otherwise misusing prompts, code or keys beyond its own published policy;
- serving a different model than the one named, without disclosure;
- phishing, imitating the CC Switch brand, or imitating another provider;
- refusing refunds owed under its own refund policy, or disappearing with prepaid balances;
- malware, or endpoints that do not use HTTPS.

A sponsor is **demoted or removed at the next release** when:

- the sponsorship ends or is not renewed;
- the review in section 4 fails and is not fixed within 30 days of notice;
- repeated, unresolved outages or user complaints make the listing misleading.

Each removal or demotion is recorded with its date and a one-line reason in the changelog under a consistent heading (for example "Presets"), so users of the affected relay can find it. Users who already added that provider keep their provider; only the preset and the README entry are removed.

## 6. Reporting a sponsor

Users can report a sponsor by opening an issue (or, for a security or data-misuse problem, a private advisory as described in [SECURITY.md](../SECURITY.md)). Reports about a sponsor are handled by a maintainer who does not manage that sponsorship, where possible.

## 7. Contact

Sponsorship enquiries currently go to the maintainer's personal email address in the README. A dedicated address or a form would make enquiries easier to track and hand over.

## Open questions for the maintainers

- Should partner presets be hidden behind a "show sponsored presets" filter, or only labelled?
- Should `primePartner` pinning be limited to a fixed number of slots?
- Who, besides the main maintainer, reviews sponsor PRs?
- Should the in-app About page link to this policy once it is adopted?
