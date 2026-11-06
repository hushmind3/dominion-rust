import {useEffect} from 'react'
import {useRemote} from '../components/UI'

export type JobState = {id: string; status: string; error?: string}
export const jobFinished = (status?: string) =>
 status !== undefined && ['completed', 'failed', 'cancelled', 'needs_review', 'interrupted'].includes(status)

export function useJob<T extends JobState>(endpoint: string | null, initial: T | null = null) {
 const remote = useRemote<T | null>(endpoint, null, false)
 const value = remote.value ?? initial
 const finished = jobFinished(value?.status)
 useEffect(() => {
  if (!endpoint || finished) return
  let live = true
  let timer: ReturnType<typeof setTimeout>
  const schedule = () => {
   timer = setTimeout(async () => {
    await remote.reload()
    if (live) schedule()
   }, 1800)
  }
  schedule()
  return () => {live = false; clearTimeout(timer)}
 }, [endpoint, finished, remote.reload])
 return {...remote, value}
}
