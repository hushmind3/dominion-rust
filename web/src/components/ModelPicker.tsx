import {useEffect, useMemo, useRef, useState} from 'react'
import {createPortal} from 'react-dom'
import {Check, ChevronDown, LoaderCircle, Search, X} from 'lucide-react'
import {get} from '../api'

type ModelOption = {id: string; name?: string}

type Props = {
  providerId: string
  value: string
  onChange: (value: string) => void
  placeholder?: string
  className?: string
  disabled?: boolean
  ariaLabel?: string
}

type Position = {top?: number; bottom?: number; left: number; width: number}

export function ModelPicker({
  providerId,
  value,
  onChange,
  placeholder = '모델 선택',
  className = '',
  disabled = false,
  ariaLabel = '모델 선택',
}: Props) {
  const [open, setOpen] = useState(false)
  const [models, setModels] = useState<ModelOption[]>([])
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [position, setPosition] = useState<Position | null>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const panel = useRef<HTMLDivElement>(null)
  const search = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!providerId) {
      setModels([])
      return
    }
    let live = true
    setLoading(true)
    setError('')
    setModels([])
    get<ModelOption[]>('/api/providers/' + providerId + '/models')
      .then((items) => {
        if (!live) return
        setModels(items)
        if (value && !items.some((item) => item.id === value)) onChange('')
      })
      .catch((reason) => {
        if (!live) return
        setError((reason as Error).message)
        setModels([])
      })
      .finally(() => {
        if (live) setLoading(false)
      })
    return () => {
      live = false
    }
    // value/onChange are intentionally not dependencies: changing the selected
    // model must not refetch the catalogue.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerId])

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent) => {
      if (
        !panel.current?.contains(event.target as Node) &&
        !trigger.current?.contains(event.target as Node)
      ) {
        setOpen(false)
      }
    }
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    const place = () => {
      const rect = trigger.current?.getBoundingClientRect()
      if (!rect) return
      const width = Math.max(rect.width, 340)
      const left = Math.min(rect.left, window.innerWidth - width - 12)
      const below = window.innerHeight - rect.bottom
      setPosition(
        below < 320 && rect.top > below
          ? {bottom: window.innerHeight - rect.top + 4, left, width}
          : {top: rect.bottom + 4, left, width},
      )
    }
    place()
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', key)
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)
    const focus = window.setTimeout(() => search.current?.focus(), 0)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', key)
      window.removeEventListener('resize', place)
      window.removeEventListener('scroll', place, true)
      window.clearTimeout(focus)
    }
  }, [open])

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (!needle) return models
    return models.filter(
      (model) =>
        model.id.toLowerCase().includes(needle) ||
        (model.name || '').toLowerCase().includes(needle),
    )
  }, [models, query])

  const choose = (id: string) => {
    onChange(id)
    setOpen(false)
    setQuery('')
  }

  return (
    <>
      <button
        ref={trigger}
        type="button"
        disabled={disabled || !providerId}
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
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

      {open &&
        position &&
        createPortal(
          <div
            ref={panel}
            role="listbox"
            aria-label={ariaLabel}
            style={{
              position: 'fixed',
              top: position.top,
              bottom: position.bottom,
              left: position.left,
              width: position.width,
              zIndex: 120,
            }}
            className="overflow-hidden rounded-xl border border-[#DDD3C5] bg-white shadow-2xl"
          >
            <div className="flex items-center gap-2 border-b border-[#EFE9DF] px-3 py-2">
              <Search size={14} className="shrink-0 text-[#9C9388]" />
              <input
                ref={search}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="모델 검색"
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

            <div className="max-h-[min(58vh,320px)] overflow-y-auto p-1">
              {loading && (
                <div className="flex items-center gap-2 px-3 py-4 text-[12px] text-[#8A8176]">
                  <LoaderCircle size={14} className="animate-spin" /> 모델 목록 불러오는 중
                </div>
              )}
              {!loading && error && (
                <div className="px-3 py-4 text-[12px] text-rose-600">{error}</div>
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
                    {model.id === value && <Check size={13} className="shrink-0" />}
                  </button>
                ))}
              {!loading && !error && filtered.length === 0 && (
                <div className="px-3 py-4 text-[12px] text-[#8A8176]">
                  {models.length ? '검색 결과가 없습니다.' : '이 제공자에서 모델을 찾지 못했습니다.'}
                </div>
              )}
            </div>

            <div className="flex items-center justify-between border-t border-[#EFE9DF] px-3 py-1.5 text-[10px] text-[#9C9388]">
              <span>{filtered.length}개 표시</span>
              <span>전체 {models.length}개</span>
            </div>
          </div>,
          document.body,
        )}
    </>
  )
}
