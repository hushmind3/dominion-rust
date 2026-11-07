import {useEffect, useId, useLayoutEffect, useMemo, useRef, useState} from 'react'
import {createPortal} from 'react-dom'
import {Check, ChevronDown, LoaderCircle, Search, X} from 'lucide-react'
import {useProviderModels} from '../hooks/useProviderModels'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {t,useLocale} from '../i18n'

type Catalogue = ReturnType<typeof useProviderModels>

type Props = {
  providerId: string
  value: string
  onChange: (value: string) => void
  placeholder?: string
  className?: string
  disabled?: boolean
  catalogue?: Catalogue
  ariaLabel?: string
}

export function ModelPicker({
  providerId,
  value,
  onChange,
  placeholder = t('picker.placeholder'),
  className = '',
  disabled = false,
  ariaLabel = t('picker.placeholder'),
  catalogue,
}: Props) {
  useLocale()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(-1)
  const listId = useId()
  const list = useRef<HTMLDivElement>(null)
  const highlighted = useRef<HTMLButtonElement>(null)
  const [freeOnly, setFreeOnly] = useState(false)
  const ownCatalogue = useProviderModels(catalogue ? '' : providerId)
  const {value: models, loading, error} = catalogue ?? ownCatalogue
  const visible = open && !!providerId && !disabled
  const {trigger, panel, position} = useAnchoredPanel(visible, () => setOpen(false), 340, 'bottom', true)
  const search = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (visible && position) search.current?.focus({preventScroll: true})
  }, [visible, Boolean(position)])
  useEffect(() => {
    setOpen(false)
    setQuery('')
    trigger.current?.focus({preventScroll: true})
  }, [providerId, disabled])

  const filtered = useMemo(() => {
    const source = freeOnly ? models.filter((model) => model.free) : models
    const needle = query.trim().toLowerCase()
    if (!needle) return source
    return source.filter(
      (model) =>
        model.id.toLowerCase().includes(needle) ||
        (model.name || '').toLowerCase().includes(needle),
    )
  }, [models, query, freeOnly])

  useEffect(() => setActive(-1), [query, freeOnly, providerId, visible])
  useLayoutEffect(() => {
    const scroll = list.current, option = highlighted.current
    if (!scroll || !option) return
    const bounds = scroll.getBoundingClientRect(), row = option.getBoundingClientRect()
    if (row.top < bounds.top) scroll.scrollTop -= bounds.top - row.top
    else if (row.bottom > bounds.bottom) scroll.scrollTop += row.bottom - bounds.bottom
  }, [active])

  const choose = (id: string) => {
    onChange(id)
    setOpen(false)
    setQuery('')
    trigger.current?.focus({preventScroll: true})
  }

  return (
    <>
      <button
        ref={trigger}
        type="button"
        disabled={disabled || !providerId}
        aria-label={ariaLabel}
        aria-haspopup="dialog"
        aria-expanded={visible}
        onClick={() => setOpen((current) => !current)}
        className={
          'inline-flex min-w-0 items-center justify-between gap-2 rounded-lg border border-[#E1D9CC] bg-[#FFFDF9] px-2.5 py-1.5 text-left text-[12px] text-[#3A342D] hover:border-[#D3C8B8] disabled:cursor-not-allowed disabled:opacity-50 ' +
          className
        }
      >
        <span className={'truncate ' + (value ? '' : 'text-[#A79C8E]')}>
          {value || placeholder}
        </span>
        <ChevronDown size={13} className="shrink-0 text-[#8A8176]" />
      </button>

      {visible &&
        position &&
        createPortal(
          <div
            ref={panel}
            role="dialog"
            aria-label={ariaLabel}
            style={position}
            className="z-[120] flex flex-col overflow-hidden rounded-xl border border-[#DDD3C5] bg-white shadow-2xl"
          >
            <div className="flex items-center gap-2 border-b border-[#EFE9DF] px-3 py-2">
              <Search size={14} className="shrink-0 text-[#9C9388]" />
              <input
                ref={search}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                role="combobox"
                aria-autocomplete="list"
                aria-expanded={visible}
                aria-controls={listId}
                aria-activedescendant={filtered[active] ? listId+'-'+active : undefined}
                aria-label={t('picker.searchAria')}
                onKeyDown={event=>{
                  if(event.nativeEvent.isComposing||loading||error||!filtered.length)return
                  if(event.key==='ArrowDown'||event.key==='ArrowUp'){
                    event.preventDefault()
                    const next=event.key==='ArrowDown'?active+1:active<0?filtered.length-1:active-1
                    setActive((next+filtered.length)%filtered.length)
                  }else if(event.key==='Enter'){
                    const selected=filtered[active]??filtered.find(item=>item.id===query.trim())
                    if(selected){event.preventDefault();choose(selected.id)}
                  }
                }}
                placeholder={t('picker.searchPlaceholder')}
                className="min-w-0 flex-1 border-0 bg-transparent text-[12px] text-[#3A342D] outline-none placeholder:text-[#A79C8E]"
              />
              {query && (
                <button
                  type="button"
                  aria-label={t('picker.clear')}
                  onClick={() => setQuery('')}
                  className="rounded p-0.5 text-[#9C9388] hover:bg-[#F3EEE5]"
                >
                  <X size={13} />
                </button>
              )}
            </div>

            <div ref={list} id={listId} role="listbox" aria-label={ariaLabel} className="min-h-0 max-h-[min(58vh,320px)] overflow-y-auto overscroll-contain p-1">
              {loading && (
                <div className="flex items-center gap-2 px-3 py-4 text-[12px] text-[#8A8176]">
                  <LoaderCircle size={14} className="animate-spin" /> {t('picker.loading')}
                </div>
              )}
              {!loading && error && (
                <div className="px-3 py-4 text-[12px] text-rose-600">{error}<button type="button" className="ml-2 underline" onClick={()=>void (catalogue??ownCatalogue).reload()}>{t('picker.retry')}</button></div>
              )}
              {!loading &&
                !error &&
                filtered.map((model, index) => (
                  <button
                    key={model.id}
                    type="button"
                    role="option"
                    id={listId+'-'+index}
                    ref={index===active?highlighted:undefined}
                    aria-selected={model.id === value}
                    onClick={() => choose(model.id)}
                    className={
                      'flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] hover:bg-[#F7F2EA] ' +
                      (index===active||model.id === value ? 'bg-[#FBF1EC] text-[#A94E30]' : 'text-[#3A342D]')
                    }
                  >
                    <span className="min-w-0 flex-1 truncate">{model.id}</span>
                    {model.free && (
                      <span className="shrink-0 rounded bg-emerald-50 px-1.5 py-0.5 text-[9px] font-semibold text-emerald-700">
                        {t('picker.free')}
                      </span>
                    )}
                    {model.id === value && <Check size={13} className="shrink-0" />}
                  </button>
                ))}
              {!loading && !error && filtered.length === 0 && (
                <div className="px-3 py-4 text-[12px] text-[#8A8176]">
                  {!models.length
                    ? t('picker.noneProvider')
                    : freeOnly && !models.some((model) => model.free)
                      ? t('picker.noneFree')
                      : t('picker.noResults')}
                </div>
              )}
            </div>

            {!freeOnly && query.trim() && !models.some(model=>model.id===query.trim()) && (
              <button type="button" disabled={new TextEncoder().encode(query.trim()).length>256}
                className="shrink-0 truncate border-t border-[#EFE9DF] px-3 py-2 text-left text-[11px] font-semibold text-[#A94E30] hover:bg-[#F7F2EA] disabled:opacity-50"
                title={query.trim()} onClick={()=>choose(query.trim())}>
                {t('picker.useId', { id: query.trim() })}
              </button>
            )}

            <div className="flex shrink-0 items-center justify-between gap-2 border-t border-[#EFE9DF] px-3 py-1.5 text-[10px] text-[#9C9388]">
              <label className="flex cursor-pointer items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={freeOnly}
                  onChange={(event) => setFreeOnly(event.target.checked)}
                  className="accent-[#C96442]"
                />
                {t('picker.freeOnly')}
              </label>
              <span>
                {t('picker.count', { shown: filtered.length, total: models.length })}
              </span>
            </div>
          </div>,
          document.body,
        )}
    </>
  )
}
