import { describe, expect, it } from 'vitest'
import { isLocale, languageOptions, resolveLocale, translate, validateCatalogs } from '../src/lib/i18n'
import { decodeWorkspace, defaultPreferences } from '../src/lib/workspace'
describe('appearance and localization', () => {
  it('keeps locale catalogs complete and preserves interpolation', () => {
    expect(validateCatalogs()).toEqual([])
    expect(languageOptions.map(x => x.code)).toEqual(['en','zh-CN','zh-TW','ja','ko','de','fr','es','pt-BR','ru','ms','id'])
    for (const { code } of languageOptions) {
      expect(isLocale(code)).toBe(true)
      expect(translate(code, 'Saved {0}', 'a.json')).toContain('a.json')
      expect(translate(code, 'Apply {0} operations', 5)).toContain('5')
    }
  })
  it('uses supported OS locales and safe fallbacks', () => {
    expect(resolveLocale('system', 'zh-Hans-CN')).toBe('zh-CN')
    expect(resolveLocale('system', 'en-GB')).toBe('en')
    expect(resolveLocale('system', 'zh-Hant-HK')).toBe('zh-TW')
    expect(resolveLocale('system', 'zh-TW')).toBe('zh-TW')
    expect(resolveLocale('system', 'pt-PT')).toBe('pt-BR')
    expect(resolveLocale('system', 'ja-JP')).toBe('ja')
    expect(resolveLocale('system', 'es-MX')).toBe('es')
    expect(resolveLocale('system', 'ms-MY')).toBe('ms')
    expect(resolveLocale('system', 'id-ID')).toBe('id')
    expect(resolveLocale('system', 'ru-RU')).toBe('ru')
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
