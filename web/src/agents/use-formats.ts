import { useTranslation } from '@floorwalker/i18n'

import { compactTokens } from './format'

/** The panel's number, time and share formats in the reader's locale, with `t`:
    the one place the views format figures. */
export function useFormats() {
 const { t, i18n } = useTranslation()
 const locale = i18n.language
 const percent = (fraction: number, digits: number) =>
  new Intl.NumberFormat(locale, {
   style: 'percent',
   maximumFractionDigits: digits,
  }).format(fraction)
 return {
  t,
  percent,
  /** A share of a range: one decimal below 10%, none above. */
  share: (fraction: number) => percent(fraction, fraction < 0.1 ? 1 : 0),
  count: (n: number) => new Intl.NumberFormat(locale).format(n),
  tokens: (n: number) => compactTokens(n, locale),
  seconds: (ms: number) =>
   new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: 'second',
    unitDisplay: 'narrow',
    maximumFractionDigits: 1,
   }).format(ms / 1000),
  /** A time of day today, otherwise a date and time. */
  dateTime: (at: number) => {
   const when = new Date(at)
   const sameDay = when.toDateString() === new Date().toDateString()
   return new Intl.DateTimeFormat(
    locale,
    sameDay
     ? { hour: '2-digit', minute: '2-digit' }
     : { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' },
   ).format(when)
  },
  /** The start of a chart bar: its hour, or its day. */
  barStart: (at: number, bucket: 'hour' | 'day') =>
   new Intl.DateTimeFormat(
    locale,
    bucket === 'hour'
     ? { hour: '2-digit', minute: '2-digit' }
     : { month: 'short', day: 'numeric' },
   ).format(new Date(at)),
  /** This month's name. */
  month: () =>
   new Intl.DateTimeFormat(locale, { month: 'long' }).format(new Date()),
 }
}
