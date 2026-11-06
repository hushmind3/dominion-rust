import {useEffect, useMemo, useRef, useState} from 'react'
import {createPortal} from 'react-dom'
import {Check, ChevronDown, LoaderCircle, Search, X} from 'lucide-react'
import {useProviderModels} from '../hooks/useProviderModels'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'

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
  placeholder = '모델 선택',
  className = '',
  disabled = false,
  ariaLabel = '모델 선택',
  catalogue,
}: Props) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
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
                aria-label="모델 검색 또는 모델 ID 입력"
                placeholder="모델 검색 또는 ID 입력"
                className="min-w-0 flex-1 border-0 bg-transparent text-[12px] text-[#3A342D] outline-none placeholder:text-[#A79C8E]"
              />
              {query && (
                <button
                  type="button"
                  aria-label="검색 지우기"
                  onClick={() => setQuery('')}
                  className="rounded p-0.5 text-[#9C9388] hover:bg-[#F3EEE5]"
                >
                  <X size={13} />
                </button>
              )}
            </div>

            <div role="listbox" aria-label={ariaLabel} className="min-h-0 max-h-[min(58vh,320px)] overflow-y-auto overscroll-contain p-1">
              {loading && (
                <div className="flex items-center gap-2 px-3 py-4 text-[12px] text-[#8A8176]">
                  <LoaderCircle size={14} className="animate-spin" /> 모델 목록 불러오는 중
                </div>
              )}
              {!loading && error && (
                <div className="px-3 py-4 text-[12px] text-rose-600">{error}<button type="button" className="ml-2 underline" onClick={()=>void (catalogue??ownCatalogue).reload()}>다시 불러오기</button></div>
              )}
              {!loading &&
                !error &&
                filtered.map((model) => (
                  <button
                    key={model.id}
                    type="button"
                    role="option"
                    aria-selected={model.id === value}
                    onClick={() => choose(model.id)}
                    className={
                      'flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] hover:bg-[#F7F2EA] ' +
                      (model.id === value ? 'bg-[#FBF1EC] text-[#A94E30]' : 'text-[#3A342D]')
                    }
                  >
                    <span className="min-w-0 flex-1 truncate">{model.id}</span>
                    {model.free && (
                      <span className="shrink-0 rounded bg-emerald-50 px-1.5 py-0.5 text-[9px] font-semibold text-emerald-700">
                        무료
                      </span>
                    )}
                    {model.id === value && <Check size={13} className="shrink-0" />}
                  </button>
                ))}
              {!loading && !error && filtered.length === 0 && (
                <div className="px-3 py-4 text-[12px] text-[#8A8176]">
                  {!models.length
                    ? '이 제공자에서 모델을 찾지 못했습니다.'
                    : freeOnly && !models.some((model) => model.free)
                      ? '이 제공자에서 무료로 표시된 모델이 없습니다.'
                      : '검색 결과가 없습니다.'}
                </div>
              )}
            </div>

            {!freeOnly && query.trim() && !models.some(model=>model.id===query.trim()) && (
              <button type="button" disabled={new TextEncoder().encode(query.trim()).length>256}
                className="shrink-0 truncate border-t border-[#EFE9DF] px-3 py-2 text-left text-[11px] font-semibold text-[#A94E30] hover:bg-[#F7F2EA] disabled:opacity-50"
                title={query.trim()} onClick={()=>choose(query.trim())}>
                이 모델 ID 사용 · {query.trim()}
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
                무료만
              </label>
              <span>
                {filtered.length}개 표시 · 전체 {models.length}개
              </span>
            </div>
          </div>,
          document.body,
        )}
    </>
  )
}
