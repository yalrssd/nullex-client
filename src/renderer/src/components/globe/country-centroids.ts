// ISO 3166-1 alpha-2 country centroids (lat, lon) for positioning markers on the globe
export const countryCentroids: Record<string, [number, number]> = {
  AD: [42.5, 1.6],
  AE: [24, 54],
  AF: [33, 65],
  AG: [17.05, -61.8],
  AI: [18.25, -63.1667],
  AL: [41, 20],
  AM: [40, 45],
  AO: [-12.5, 18.5],
  AR: [-34, -64],
  AT: [47.3333, 13.3333],
  AU: [-27, 133],
  AZ: [40.5, 47.5],
  BA: [44, 18],
  BD: [24, 90],
  BE: [50.8333, 4],
  BG: [43, 25],
  BH: [26, 50.55],
  BR: [-10, -55],
  BY: [53, 28],
  CA: [60, -95],
  CH: [47, 8],
  CL: [-30, -71],
  CN: [35, 105],
  CO: [4, -72],
  CY: [35, 33],
  CZ: [49.75, 15.5],
  DE: [51, 9],
  DK: [56, 10],
  EE: [59, 26],
  EG: [27, 30],
  ES: [40, -4],
  FI: [64, 26],
  FR: [46, 2],
  GB: [54, -2],
  GE: [42, 43.5],
  GR: [39, 22],
  HK: [22.25, 114.1667],
  HR: [45.1667, 15.5],
  HU: [47, 20],
  ID: [-5, 120],
  IE: [53, -8],
  IL: [31.5, 34.75],
  IN: [20, 77],
  IS: [65, -18],
  IT: [42.8333, 12.8333],
  JP: [36, 138],
  KR: [37, 127.5],
  KZ: [48, 68],
  LT: [56, 24],
  LU: [49.75, 6.1667],
  LV: [57, 25],
  MD: [47, 29],
  ME: [42, 19],
  MK: [41.8333, 22],
  MX: [23, -102],
  MY: [2.5, 112.5],
  NL: [52.5, 5.75],
  NO: [62, 10],
  NZ: [-41, 174],
  PL: [52, 20],
  PT: [39.5, -8],
  RO: [46, 25],
  RS: [44, 21],
  RU: [60, 100],
  SE: [62, 15],
  SG: [1.3667, 103.8],
  SI: [46, 15],
  SK: [48.6667, 19.5],
  TH: [15, 100],
  TR: [39, 35],
  TW: [23.5, 121],
  UA: [49, 32],
  US: [38, -97],
  VN: [16, 106],
  ZA: [-29, 24]
}

const REGIONAL_INDICATOR_BASE = 0x1f1e6
const UPPERCASE_A = 0x41

/**
 * Extracts ISO-2 code from a flag emoji string (e.g. "🇩🇪" -> "DE")
 */
export function isoCodeFromFlagEmoji(emoji?: string | null): string | null {
  if (!emoji) return null
  const runes = Array.from(emoji)
  for (let i = 0; i < runes.length - 1; i++) {
    const c1 = runes[i].codePointAt(0)
    const c2 = runes[i + 1].codePointAt(0)
    if (
      c1 &&
      c2 &&
      c1 >= REGIONAL_INDICATOR_BASE &&
      c1 <= REGIONAL_INDICATOR_BASE + 25 &&
      c2 >= REGIONAL_INDICATOR_BASE &&
      c2 <= REGIONAL_INDICATOR_BASE + 25
    ) {
      const l1 = String.fromCharCode(UPPERCASE_A + (c1 - REGIONAL_INDICATOR_BASE))
      const l2 = String.fromCharCode(UPPERCASE_A + (c2 - REGIONAL_INDICATOR_BASE))
      return `${l1}${l2}`
    }
  }
  return null
}

/**
 * Try to deduce an ISO country code from a server name if no flag emoji is present
 */
export function detectIsoCodeFromName(name: string): string | null {
  const fromEmoji = isoCodeFromFlagEmoji(name)
  if (fromEmoji) return fromEmoji

  const upper = name.toUpperCase()
  const patterns: [RegExp, string][] = [
    [/(\b(GERMANY|DE|FRANKFURT|BERLIN)\b|ГЕРМАНИ|ФРАНКФУРТ)/i, 'DE'],
    [/(\b(NETHERLANDS|NL|AMSTERDAM)\b|НИДЕРЛАНД|ГОЛЛАНД|АМСТЕРДАМ)/i, 'NL'],
    [/(\b(UNITED STATES|USA|US|AMERICA|NEW YORK|CALIFORNIA|LOS ANGELES|MIAMI|DALLAS|SEATTLE|CHICAGO)\b|США|АМЕРИК)/i, 'US'],
    [/(\b(UNITED KINGDOM|UK|LONDON|ENGLAND|BRITAIN)\b|ВЕЛИКОБРИТАН|АНГЛИ|ЛОНДОН)/i, 'GB'],
    [/(\b(RUSSIA|RU|MOSCOW|SPB)\b|РОССИ|МОСКВ|ПИТЕР)/i, 'RU'],
    [/(\b(FRANCE|FR|PARIS)\b|ФРАНЦИ|ПАРИЖ)/i, 'FR'],
    [/(\b(FINLAND|FI|HELSINKI)\b|ФИНЛЯНДИ|ХЕЛЬСИНК)/i, 'FI'],
    [/(\b(SWEDEN|SE|STOCKHOLM)\b|ШВЕЦИ|СТОКГОЛЬМ)/i, 'SE'],
    [/(\b(TURKEY|TR|ISTANBUL)\b|ТУРЦИ|СТАМБУЛ)/i, 'TR'],
    [/(\b(KAZAKHSTAN|KZ|ALMATY|ASTANA)\b|КАЗАХСТАН|АЛМАТ|АСТАН)/i, 'KZ'],
    [/(\b(SINGAPORE|SG)\b|СИНГАПУР)/i, 'SG'],
    [/(\b(JAPAN|JP|TOKYO)\b|ЯПОНИ|ТОКИО)/i, 'JP'],
    [/(\b(POLAND|PL|WARSAW)\b|ПОЛЬШ|ВАРШАВ)/i, 'PL'],
    [/(\b(UKRAINE|UA|KYIV)\b|УКРАИН|КИЕВ)/i, 'UA'],
    [/(\b(CANADA|CA|TORONTO)\b|КАНАД|ТОРОНТ)/i, 'CA'],
    [/(\b(SWITZERLAND|CH|ZURICH)\b|ШВЕЙЦАР|ЦЮРИХ)/i, 'CH'],
    [/(\b(AUSTRIA|AT|VIENNA)\b|АВСТРИ|ВЕН[АЕ])/i, 'AT'],
    [/(\b(SPAIN|ES|MADRID)\b|ИСПАНИ|МАДРИД)/i, 'ES'],
    [/(\b(ITALY|IT|MILAN|ROME)\b|ИТАЛИ|МИЛАН|РИМ)/i, 'IT'],
    [/(\b(HONG KONG|HK)\b|ГОНКОНГ)/i, 'HK'],
    [/(\b(TAIWAN|TW)\b|ТАЙВАН)/i, 'TW'],
    [/(\b(KOREA|KR|SEOUL)\b|КОРЕ|СЕУЛ)/i, 'KR'],
    [/(\b(LATVIA|LV|RIGA)\b|ЛАТВИ|РИГ)/i, 'LV'],
    [/(\b(ESTONIA|EE|TALLINN)\b|ЭСТОНИ|ТАЛЛИН)/i, 'EE'],
    [/(\b(LITHUANIA|LT|VILNIUS)\b|ЛИТВ|ВИЛЬНЮС)/i, 'LT'],
    [/(\b(NORWAY|NO|OSLO)\b|НОРВЕГИ|ОСЛО)/i, 'NO'],
    [/(\b(CZECH|CZ|PRAGUE)\b|ЧЕХИ|ПРАГ)/i, 'CZ']
  ]

  for (const [re, code] of patterns) {
    if (re.test(upper)) return code
  }
  return null
}
