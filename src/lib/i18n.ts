import type { Locale, Language } from '../contracts'
import { english, simplified } from '../locales/catalog'

const catalogs = import.meta.glob<string[]>('../locales/*.json', { eager: true, import: 'default' })
const index = new Map<string, number>(english.map((key, i) => [key, i] as const))
const dictionaries: Record<string, readonly string[]> = Object.assign(Object.create(null), { en: english, 'zh-CN': simplified })
for (const [path, values] of Object.entries(catalogs)) {
  const locale = path.split('/').pop()!.replace(/\.json$/, '')
  if (values.length === english.length && values.every(s => typeof s === 'string' && s.length > 0)) dictionaries[locale] = values
}
const names: Record<Language, string> = {
  en: 'English', 'zh-CN': '简体中文', 'zh-TW': '繁體中文', ja: '日本語', ko: '한국어',
  de: 'Deutsch', fr: 'Français', es: 'Español', 'pt-BR': 'Português (Brasil)', ru: 'Русский',
  ms: 'Bahasa Melayu', id: 'Bahasa Indonesia',
}
export const languageOptions = (Object.keys(names) as Language[]).filter(code => dictionaries[code]).map(code => ({ code, name: names[code] }))
export const isLocale = (value: string): value is Locale => value === 'system' || languageOptions.some(option => option.code === value)
export function resolveLocale(value: Locale, system = 'en'): Language {
  if (value !== 'system') return dictionaries[value] ? value : 'en'
  const tag = system.toLowerCase().replace(/_/g, '-')
  const primary = tag.split('-')[0]
  if (primary === 'zh') return dictionaries['zh-TW'] && /(?:hant|tw|hk|mo)(?:-|$)/.test(tag) ? 'zh-TW' : 'zh-CN'
  if (primary === 'pt' && dictionaries['pt-BR']) return 'pt-BR'
  return (dictionaries[primary] ? primary : 'en') as Language
}
export function translate(language: Language, key: string, ...args: (string | number)[]): string {
  const i = index.get(key)
  const message = i === undefined ? key : (dictionaries[language]?.[i] ?? english[i]!)
  return message.replace(/\{(\d+)\}/g, (match, n: string) => args[Number(n)] === undefined ? match : String(args[Number(n)]))
}
export function validateCatalogs(): string[] {
  const errors: string[] = []
  for (const [code, values] of Object.entries(dictionaries)) {
    if (values.length !== english.length) errors.push(code + ': wrong length')
    english.forEach((key, i) => {
      const placeholders = (s: string) => (s.match(/\{\d+\}/g) || []).sort().join(',')
      if (!values[i] || placeholders(key) !== placeholders(values[i]!)) errors.push(code + ': ' + key)
    })
  }
  for (const path of Object.keys(catalogs)) if (!dictionaries[path.split('/').pop()!.replace(/\.json$/, '')]) errors.push(path + ': invalid catalog')
  return errors
}
