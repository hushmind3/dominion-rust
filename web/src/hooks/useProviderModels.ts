import {useRemote} from '../components/UI'
import type {ModelOption} from '../api'

export function useProviderModels(providerId: string) {
 return useRemote<ModelOption[]>(providerId ? '/api/providers/' + encodeURIComponent(providerId) + '/models' : null, [], false)
}
