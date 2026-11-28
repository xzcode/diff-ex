# Appearance and localization / 主题与国际化

## Appearance

- Pure black (`black`), neutral gray (`gray`), pure white (`white`), soft warm white (`light`), and original midnight (`dark`).
- The original stored IDs are preserved. Older `light` preferences continue to mean soft white; `white` is a new independent palette.
- All palettes include distinct shell, sidebar, toolbar, control, table, modal, editor and diff colors. The pure white theme uses a #ffffff editor; soft white uses #faf8f3.
- Compact desktop header and tabs, consistent 34px primary controls, cleaner overview, visible active navigation, responsive wrapping for longer translations.
- No changes to the comparison algorithms or file/sync safety rules.

## Languages

English, Simplified Chinese, Traditional Chinese, Japanese, Korean, German, French, Spanish, Brazilian Portuguese, Russian, Malay, and Indonesian. The system option resolves regional tags (e.g. zh-Hant-HK → zh-TW, pt-PT → pt-BR, ms-MY → ms). Unknown system languages fall back to English.

The application-owned catalog contains 473 message identifiers with complete translations in each supported language. User-provided text, names, paths, technical format identifiers and raw engine diagnostics are not translated. OS dialogs and Monaco's built-in commands may use their own language; app-specific Monaco commands are re-registered on language change without recreating editor models.

To add a locale, append new message identifiers to `src/locales/catalog.ts` without changing existing positions, supply a complete array in `src/locales/<locale>.json`, register the locale in `contracts.ts` and `lib/i18n.ts`, and run `npm test`. The validation checks catalog completeness and numbered interpolation arguments. Do not silently fall back to English for incomplete published locales.

## Verification

The updated localization unit tests check complete catalogs, interpolation, locale resolution and persisted theme/locale combinations. The production UI smoke workflow exercises all 12 language settings, checks the displayed dialog label and persistence, applies each theme to an existing comparison, checks the Monaco editor background, and captures theme screenshots. See the actual GitHub Actions results for pass/fail evidence. Native platform installation and linguistic review by native speakers are separate verification tasks.

README.md remains the default English landing page, with README.zh-CN.md available from its language switch.
