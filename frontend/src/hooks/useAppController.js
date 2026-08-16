import { useAppData } from './useAppData.js'
import { useAppActions } from './useAppActions.js'

export function useAppController() {
  const data = useAppData()
  const actions = useAppActions(data)
  return { ...data, ...actions }
}
