import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'

export default function ImmersiveAgentPaused() {
  const navigate = useNavigate()
  useEffect(() => { navigate('/agent', { replace: true }) }, [navigate])
  return null
}
