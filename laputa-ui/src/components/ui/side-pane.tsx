import { useState, useRef, useEffect } from 'react'
import { ChevronLeft, Settings, Play, Pause, ChevronUp, ChevronDown, ArrowUp } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useChatContext } from '@/hooks/use-chat-context'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'

interface SidePaneProps {
  children?: React.ReactNode
  className?: string
}

export function SidePane({ children, className }: SidePaneProps) {
  const { messages, sendMessage, status, isPaused, setIsPaused, idleOpacity, setIdleOpacity } = useChatContext()

  const [isOpen, setIsOpen] = useState(false)
  const [showSettings, setShowSettings] = useState(false)
  const [arrowY, setArrowY] = useState(() => {
    const saved = localStorage.getItem('arrow-position-y')
    if (!saved) return 32
    const parsed = parseFloat(saved)
    return isNaN(parsed) ? 32 : parsed
  })
  const [isDraggingArrow, setIsDraggingArrow] = useState(false)
  const [showChat, setShowChat] = useState(true)
  const [input, setInput] = useState('')

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const dragStartRef = useRef<{ mouseY: number; posY: number } | null>(null)

  const isLoading = status === 'streaming' || status === 'submitted'
  const hasMessages = messages.length > 0

  useEffect(() => {
    if (showChat && messagesEndRef.current) messagesEndRef.current.scrollIntoView({ behavior: 'smooth' })
  }, [messages, showChat])

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto'
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 80)}px`
    }
  }, [input])

  const handleArrowDragStart = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setIsDraggingArrow(true)
    dragStartRef.current = { mouseY: e.clientY, posY: arrowY }
  }

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (isDraggingArrow && dragStartRef.current) {
        const newY = Math.max(32, Math.min(window.innerHeight - 100, dragStartRef.current.posY + (e.clientY - dragStartRef.current.mouseY)))
        setArrowY(newY)
      }
    }
    const handleMouseUp = () => {
      if (isDraggingArrow) {
        setIsDraggingArrow(false)
        // Persist once on drag end, not per-pixel
        if (dragStartRef.current !== null) {
          localStorage.setItem('arrow-position-y', arrowY.toString())
        }
        dragStartRef.current = null
      }
    }
    if (isDraggingArrow) {
      document.addEventListener('mousemove', handleMouseMove)
      document.addEventListener('mouseup', handleMouseUp)
    }
    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
    }
  }, [isDraggingArrow, arrowY])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!input.trim() || isLoading) return
    sendMessage({ text: input })
    setInput('')
  }

  return (
    <>
      <button
        onClick={() => !isDraggingArrow && setIsOpen(!isOpen)}
        onMouseDown={handleArrowDragStart}
        style={{ top: `${arrowY}px`, backgroundColor: `rgba(255,255,255,${idleOpacity * 3})` }}
        className={cn(
          'fixed right-8 z-50 p-3 rounded-xl transition-all duration-300',
          'backdrop-blur-lg ring-1 ring-white/[0.06] shadow-lg shadow-black/20',
          'hover:ring-white/15',
          isDraggingArrow ? 'cursor-grabbing' : 'cursor-grab',
          isOpen && 'translate-x-[-320px]'
        )}
        aria-label={isOpen ? 'Close pane' : 'Open pane'}
      >
        <ChevronLeft className={cn('w-5 h-5 text-white/70 transition-transform duration-300', !isOpen && 'rotate-180')} />
      </button>

      <div
        style={{ backgroundColor: `rgba(20, 20, 28, ${Math.min(0.95, 0.6 + idleOpacity * 8)})` }}
        className={cn(
          'fixed top-0 right-0 h-full w-[360px] z-50 transition-transform duration-500 ease-out',
          'backdrop-blur-2xl ring-1 ring-white/10 shadow-2xl shadow-black/30',
          isOpen ? 'translate-x-0' : 'translate-x-full',
          className
        )}
      >
        <div className="flex flex-col h-full">
          <div className="p-4 border-b border-white/10 flex items-center justify-between">
            <button
              onClick={() => setIsPaused(!isPaused)}
              className={cn(
                'flex items-center gap-2 px-4 py-2 rounded-xl transition-all duration-200',
                isPaused
                  ? 'bg-primary/20 text-primary ring-1 ring-primary/30'
                  : 'bg-white/5 text-white/70 ring-1 ring-white/10 hover:bg-white/10'
              )}
            >
              {isPaused ? <><Play className="w-4 h-4" /><span className="text-sm font-medium">Resume AI</span></>
                : <><Pause className="w-4 h-4" /><span className="text-sm font-medium">Pause AI</span></>}
            </button>
            <button onClick={() => setShowSettings(true)}
              className="p-2.5 rounded-xl bg-white/5 hover:bg-white/10 ring-1 ring-white/10 transition-all duration-200"
              aria-label="Settings">
              <Settings className="w-4 h-4 text-white/60" />
            </button>
          </div>

          <div className="flex-1 flex flex-col overflow-hidden">
            <button
              onClick={() => setShowChat(!showChat)}
              className="flex items-center justify-center gap-2 py-2 text-white/30 hover:text-white/50 transition-colors border-b border-white/5"
            >
              {showChat
                ? <><ChevronDown className="w-4 h-4" /><span className="text-xs">Hide chat</span></>
                : <><ChevronUp className="w-4 h-4" /><span className="text-xs">{hasMessages ? `${messages.length} messages` : 'Show chat'}</span></>}
            </button>

            {showChat && (
              <div className="flex-1 overflow-y-auto p-4 space-y-3">
                {messages.length === 0 && <p className="text-center text-white/20 text-sm py-8">No messages yet</p>}
                {messages.map((message) => (
                  <div key={message.id} className={cn('flex', message.role === 'user' ? 'justify-end' : 'justify-start')}>
                    <div className={cn(
                      'max-w-[85%] rounded-2xl px-3 py-2 text-xs leading-relaxed',
                      message.role === 'user'
                        ? 'bg-primary text-primary-foreground'
                        : 'bg-white/5 text-foreground ring-1 ring-white/10'
                    )}>
                      {message.content}
                    </div>
                  </div>
                ))}
                {isLoading && (
                  <div className="flex justify-start">
                    <div className="bg-white/5 rounded-2xl px-3 py-2 ring-1 ring-white/10">
                      <div className="flex gap-1">
                        <span className="w-1 h-1 bg-primary/60 rounded-full animate-bounce [animation-delay:-0.3s]" />
                        <span className="w-1 h-1 bg-primary/60 rounded-full animate-bounce [animation-delay:-0.15s]" />
                        <span className="w-1 h-1 bg-primary/60 rounded-full animate-bounce" />
                      </div>
                    </div>
                  </div>
                )}
                <div ref={messagesEndRef} />
              </div>
            )}

            {showChat && (
              <form onSubmit={handleSubmit} className="p-3 border-t border-white/10">
                <div className="flex items-end gap-2">
                  <textarea
                    ref={textareaRef}
                    value={input}
                    onChange={(e) => setInput(e.target.value)}
                    onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSubmit(e) } }}
                    placeholder="Message..."
                    disabled={isLoading}
                    rows={1}
                    className="flex-1 bg-white/5 rounded-xl px-3 py-2 text-sm placeholder:text-white/20 text-foreground disabled:opacity-50 resize-none border-none outline-none ring-1 ring-white/10 focus:ring-white/20"
                  />
                  <button type="submit" disabled={!input.trim() || isLoading}
                    className={cn('p-2 rounded-xl transition-all duration-200',
                      input.trim() && !isLoading
                        ? 'bg-primary text-primary-foreground hover:brightness-110'
                        : 'bg-white/5 text-white/15 cursor-not-allowed'
                    )}>
                    <ArrowUp className="w-4 h-4" />
                  </button>
                </div>
              </form>
            )}
          </div>

          {children && <div className="p-4 border-t border-white/10">{children}</div>}
        </div>
      </div>

      <Dialog open={showSettings} onOpenChange={setShowSettings}>
        <DialogContent className="w-full max-w-md">
          <DialogHeader>
            <DialogTitle>Settings</DialogTitle>
          </DialogHeader>
          <div className="p-6 space-y-6">
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <label className="text-sm font-medium text-foreground">UI Opacity</label>
                <span className="text-xs text-muted-foreground">{Math.round(idleOpacity * 100)}%</span>
              </div>
              <input
                type="range" min="0.01" max="0.3" step="0.01" value={idleOpacity}
                onChange={(e) => setIdleOpacity(parseFloat(e.target.value))}
                className="w-full h-2 bg-white/10 rounded-full appearance-none cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-primary [&::-webkit-slider-thumb]:cursor-pointer [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-primary [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:cursor-pointer"
              />
              <p className="text-xs text-muted-foreground">Controls transparency of chat box, pane, and toggle button when idle</p>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
