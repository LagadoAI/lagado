import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'

export default function ImmersiveAgentRunning() {
  const navigate = useNavigate()
  useEffect(() => { navigate('/immersive', { replace: true }) }, [navigate])
  return null
}
