import { describe, expect, it } from 'vitest'
import { isLocale, languageOptions, resolveLocale, translate, validateCatalogs } from '../src/lib/i18n'
import { decodeWorkspace, defaultPreferences } from '../src/lib/workspace'
describe('appearance and localization', () => {
  it('keeps locale catalogs complete and preserves interpolation', () => {
    expect(validateCatalogs()).toEqual([])
    for (const { code } of languageOptions) {
      expect(isLocale(code)).toBe(true)
      expect(translate(code, 'Saved {0}', 'a.json')).toContain('a.json')
      expect(translate(code, 'Apply {0} operations', 5)).toContain('5')
    }
  })
  it('uses supported OS locales and safe fallbacks', () => {
    expect(resolveLocale('system', 'zh-Hans-CN')).toBe('zh-CN')
    expect(resolveLocale('system', 'en-GB')).toBe('en')
    expect(resolveLocale('system', 'xx-XX')).toBe('en')
  })
  it('restores all theme and language combinations without losing the workspace', () => {
    for (const theme of ['dark', 'black', 'gray', 'white', 'light']) {
      for (const locale of ['system', ...languageOptions.map(option => option.code)]) {
        const workspace = { version: 1, sessions: [], activeId: '', savedSessions: [], preferences: { ...defaultPreferences, theme, locale } }
        expect(decodeWorkspace(JSON.stringify(workspace)).preferences).toEqual(workspace.preferences)
      }
    }
  })
})
