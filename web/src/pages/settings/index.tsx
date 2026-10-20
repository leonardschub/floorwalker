import { LuChevronLeft } from 'react-icons/lu'
import { Link } from 'react-router'

import { Slider, Switch } from '@heroui/react'
import { useTranslation } from '@floorwalker/i18n'

import { useDocumentTitle } from '../../components/document-title'
import { desktopBuild } from '../../landing'
import { setPreference, useThemePreference } from '../../theme/theme'
import {
 defaultControls,
 lookSensitivityRange,
 setWorldSettings,
 subagentScaleRange,
 useWorldSettings,
 walkSpeedRange,
} from '../../world/settings'

/* Settings: Appearance, World (desktop build only) and About, all of it stored on
   this device or compiled into the app. There is no account. */

const themeOptions = [
 { value: 'system', labelKey: 'settings.appearance.theme-system' },
 { value: 'light', labelKey: 'settings.appearance.theme-light' },
 { value: 'dark', labelKey: 'settings.appearance.theme-dark' },
] as const

const groupClass = 'flex flex-col gap-2.5'
const groupTitleClass =
 'text-xs font-semibold tracking-[0.06em] text-faint uppercase'
const cardClass =
 'overflow-hidden rounded-lg border border-separator bg-surface shadow-card'
const rowClass =
 'flex flex-wrap items-center justify-between gap-x-4 gap-y-3 border-t border-separator px-5 py-4 first:border-t-0'
const rowMainClass = 'min-w-[200px] flex-1'
const rowLabelClass = 'block font-medium'
const rowMetaClass = 'mt-0.5 block text-xs text-muted'
const rowValueClass = 'mt-0.5 block text-mono text-muted'
const rowActionsClass = 'flex shrink-0 flex-wrap items-center gap-2.5'
const backClass =
 'inline-flex items-center gap-1 text-accent underline underline-offset-4 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus'
const actionClass =
 'inline-flex h-7.5 shrink-0 items-center justify-center rounded-md border border-separator px-3 text-xs font-medium text-muted hover:bg-raised hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus'
/* The World section shows only in the desktop build: the browser build has no world.
   Read where it is used, so the bundler drops the section from the browser bundle. */
const worldOptions = import.meta.env.MODE === 'desktop'

const themeOptionClass =
 'inline-flex h-7.5 items-center justify-center rounded-pill px-3 text-xs font-medium text-muted aria-pressed:bg-accent aria-pressed:text-accent-ink aria-pressed:shadow-control focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus'

function Settings() {
 const { t } = useTranslation()
 useDocumentTitle(t('title.settings'))
 const preference = useThemePreference()

 return (
  <main className="mx-auto flex w-full max-w-[720px] flex-col gap-6.5 px-6 py-10 max-[520px]:px-4 max-[520px]:py-6">
   <header>
    {/* The way back returns to the world, kept warm while Settings is open. The
        browser build opens on Settings and has no world to return to. */}
    {desktopBuild ? (
     <nav aria-label={t('settings.navigation')} className="mb-4 text-sm">
      <Link className={backClass} to="/world">
       <LuChevronLeft aria-hidden="true" />
       {t('settings.world-back')}
      </Link>
     </nav>
    ) : null}
    <h1 className="text-2xl font-semibold tracking-tight">
     {t('settings.heading')}
    </h1>
    <p className="mt-1.5 text-muted">{t('settings.lead')}</p>
   </header>

   <section className={groupClass}>
    <h2 className={groupTitleClass}>{t('settings.appearance.title')}</h2>
    <div className={cardClass}>
     <div className={rowClass}>
      <div className={rowMainClass}>
       <span className={rowLabelClass}>{t('settings.appearance.theme')}</span>
       <span className={rowMetaClass}>
        {t('settings.appearance.theme-description')}
       </span>
      </div>
      <fieldset className="m-0 inline-flex min-w-0 shrink-0 items-center gap-0.5 rounded-pill border border-separator bg-field p-0.75">
       <legend className="sr-only">{t('settings.appearance.theme')}</legend>
       {themeOptions.map((option) => (
        <button
         key={option.value}
         aria-pressed={preference === option.value}
         className={themeOptionClass}
         type="button"
         onClick={() => setPreference(option.value)}
        >
         {t(option.labelKey)}
        </button>
       ))}
      </fieldset>
     </div>
     <div className={rowClass}>
      <div className={rowMainClass}>
       <span className={rowLabelClass}>{t('settings.appearance.accent')}</span>
       <span className={rowMetaClass}>
        {t('settings.appearance.accent-description')}
       </span>
      </div>
      <p className="shrink-0 text-muted">
       {t('settings.appearance.accent-value')}
      </p>
     </div>
    </div>
   </section>

   {worldOptions ? <WorldSection /> : null}

   <AboutSection />
  </main>
 )
}

/* About (local-only spec, Legal): the product's name, the credit, and the appropriate
   legal notices the AGPL's section 0 asks an interactive program to show: the copyright
   notice, that there is no warranty, that the user may redistribute the app under the
   same licence, and a way to read the licence. The licence text and the open-source
   licences page each have their own route, so neither is in this route's chunk. The
   credit is a credit, not an imprint: the owner's name only. */
function AboutSection() {
 const { t } = useTranslation()

 return (
  <section className={groupClass} data-settings-section="about">
   <h2 className={groupTitleClass}>{t('settings.about.title')}</h2>
   <div className={cardClass}>
    <div className={rowClass}>
     <div className={rowMainClass}>
      <span className={rowLabelClass}>{t('index.wordmark')}</span>
      <span className={rowMetaClass}>{t('settings.about.credit')}</span>
      <span className={rowMetaClass}>{t('settings.about.copyright')}</span>
     </div>
    </div>
    <div className={rowClass}>
     <div className="flex min-w-[200px] flex-1 flex-col gap-2 text-xs text-muted">
      <p>{t('settings.about.free-software')}</p>
      <p>{t('settings.about.no-warranty')}</p>
     </div>
    </div>
    <div className={rowClass}>
     <div className={rowMainClass}>
      <span className={rowLabelClass}>{t('settings.about.licence')}</span>
      <span className={rowValueClass}>
       {t('settings.about.licence-identifier')}
      </span>
     </div>
     <div className={rowActionsClass}>
      <Link
       aria-label={t('settings.about.licence-open-label')}
       className={actionClass}
       to="/license"
      >
       {t('settings.about.open')}
      </Link>
     </div>
    </div>
    <div className={rowClass}>
     <div className={rowMainClass}>
      <span className={rowLabelClass}>{t('settings.about.oss-licenses')}</span>
      <span className={rowMetaClass}>
       {t('settings.about.oss-licenses-description')}
      </span>
     </div>
     <div className={rowActionsClass}>
      <Link
       aria-label={t('settings.about.oss-licenses-open-label')}
       className={actionClass}
       to="/oss-licenses"
      >
       {t('settings.about.open')}
      </Link>
     </div>
    </div>
   </div>
  </section>
 )
}

/* The world's options, device-local like the theme preference (world/settings.ts). A
   change is stored and sent to the world at once; there is nothing to save. */
function WorldSection() {
 const { t } = useTranslation()
 const world = useWorldSettings()
 const switches = [
  { key: 'textures', setting: 'textures', on: world.textures },
  { key: 'clutter', setting: 'clutter', on: world.clutter },
  {
   key: 'punishmentEffects',
   setting: 'punishment-effects',
   on: world.punishmentEffects,
  },
 ] as const

 return (
  <section className={groupClass} data-settings-section="world">
   <h2 className={groupTitleClass}>{t('settings.world.title')}</h2>
   <div className={cardClass}>
    <WorldSlider
     label={t('settings.world.subagent-size')}
     description={t('settings.world.subagent-size-description')}
     range={subagentScaleRange}
     value={world.subagentScale}
     shown={t('settings.world.subagent-size-value', {
      value: world.subagentScale.toFixed(2),
     })}
     onChange={(subagentScale) => setWorldSettings({ subagentScale })}
    />
    <WorldSlider
     setting="walk-speed"
     label={t('settings.world.walk-speed')}
     description={t('settings.world.walk-speed-description')}
     range={walkSpeedRange}
     value={world.walkSpeed}
     shown={t('settings.world.walk-speed-value', {
      value: world.walkSpeed.toFixed(1),
     })}
     onChange={(walkSpeed) => setWorldSettings({ walkSpeed })}
     onReset={
      world.walkSpeed === defaultControls.walkSpeed
       ? undefined
       : () => setWorldSettings({ walkSpeed: defaultControls.walkSpeed })
     }
    />
    <WorldSlider
     setting="look-sensitivity"
     label={t('settings.world.look-sensitivity')}
     description={t('settings.world.look-sensitivity-description')}
     range={lookSensitivityRange}
     value={world.lookSensitivity}
     shown={t('settings.world.look-sensitivity-value', {
      value: world.lookSensitivity.toFixed(1),
     })}
     onChange={(lookSensitivity) => setWorldSettings({ lookSensitivity })}
     onReset={
      world.lookSensitivity === defaultControls.lookSensitivity
       ? undefined
       : () =>
          setWorldSettings({ lookSensitivity: defaultControls.lookSensitivity })
     }
    />
    {switches.map((option) => (
     <div
      key={option.key}
      className={rowClass}
      data-world-setting={option.setting}
     >
      <div className={rowMainClass}>
       <span className={rowLabelClass}>
        {t(`settings.world.${option.setting}`)}
       </span>
       <span className={rowMetaClass}>
        {t(`settings.world.${option.setting}-description`)}
       </span>
      </div>
      <Switch
       aria-label={t(`settings.world.${option.setting}`)}
       isSelected={option.on}
       onChange={(on) => setWorldSettings({ [option.key]: on })}
      >
       <Switch.Content>
        <Switch.Control>
         <Switch.Thumb />
        </Switch.Control>
       </Switch.Content>
      </Switch>
     </div>
    ))}
   </div>
  </section>
 )
}

/* One slider row of the World section, with its value shown above the track. A row
   with `onReset` gets a Reset action, disabled while the value is its default. */
function WorldSlider(props: {
 setting?: string
 label: string
 description: string
 range: { min: number; max: number; step: number }
 value: number
 shown: string
 onChange: (value: number) => void
 onReset?: (() => void) | undefined
}) {
 const { t } = useTranslation()
 const resettable = props.setting !== undefined

 return (
  <div className={rowClass} data-world-setting={props.setting}>
   <div className={rowMainClass}>
    <span className={rowLabelClass}>{props.label}</span>
    <span className={rowMetaClass}>{props.description}</span>
   </div>
   <div className={rowActionsClass}>
    <Slider
     className="w-56 shrink-0"
     aria-label={props.label}
     minValue={props.range.min}
     maxValue={props.range.max}
     step={props.range.step}
     value={props.value}
     onChange={(value) =>
      props.onChange(Array.isArray(value) ? value[0] : value)
     }
    >
     <Slider.Output className="text-mono text-muted">
      {props.shown}
     </Slider.Output>
     <Slider.Track>
      <Slider.Fill />
      <Slider.Thumb />
     </Slider.Track>
    </Slider>
    {resettable ? (
     <button
      aria-label={t('settings.world.reset-label', { setting: props.label })}
      className={`${actionClass} disabled:opacity-50`}
      disabled={props.onReset === undefined}
      type="button"
      onClick={props.onReset}
     >
      {t('settings.world.reset')}
     </button>
    ) : null}
   </div>
  </div>
 )
}

export default Settings
