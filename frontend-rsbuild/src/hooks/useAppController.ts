import { useAppData } from './useAppData'
import { useAppActions } from './useAppActions'

export function useAppController() {
  const data = useAppData()
  const actions = useAppActions(data)
  return { ...data, ...actions }
}
