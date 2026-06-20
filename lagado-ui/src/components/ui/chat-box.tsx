import { useState, useRef, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Paperclip, Mic, MicOff, ArrowUp, X, ChevronUp, ChevronDown } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useChatContext } from '@/hooks/use-chat-context'
import { PermissionCard } from '@/components/PermissionCard'

interface ChatBoxProps {
  placeholder?: string
  className?: string
}

interface FileAttachment {
  id: string
  name: string
  size: number
  type: string
}

interface Position {
  x: number
  y: number
}

export function ChatBox({
  placeholder = 'What can I help you with?',
  className
}: ChatBoxProps) {
  const navigate = useNavigate()
  const { messages, sendMessage, status, isPaused, idleOpacity, pendingPermission, approve, deny } = useChatContext()

  const surfaceRoutes: Record<string, string> = {
    immersive: '/agent',
    agent: '/agent',
    chat: '/chat',
  }
  const handleSwitch = (surface: string) => navigate(surfaceRoutes[surface] ?? '/')

  const [input, setInput] = useState('')
  const [isActive, setIsActive] = useState(false)
  const [isExpanded, setIsExpanded] = useState(false)
  const [isRecording, setIsRecording] = useState(false)
  const [attachments, setAttachments] = useState<FileAttachment[]>([])
  const [position, setPosition] = useState<Position>(() => {
    const saved = localStorage.getItem('chatbox-position')
    if (saved) { try { return JSON.parse(saved) } catch { } }
    return { x: 0, y: 0 }
  })
  const [isDragging, setIsDragging] = useState(false)

  const fileInputRef = useRef<HTMLInputElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const dragStartRef = useRef<{ mouseX: number; mouseY: number; posX: number; posY: number } | null>(null)

  const isLoading = status === 'streaming' || status === 'submitted'
  const hasMessages = messages.length > 0

  useEffect(() => {
    localStorage.setItem('chatbox-position', JSON.stringify(position))
  }, [position])

  useEffect(() => { if (hasMessages) setIsActive(true) }, [hasMessages])

  useEffect(() => {
    if (isExpanded) messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, isExpanded])

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node) && !hasMessages && !isDragging) {
        setIsActive(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [hasMessages, isDragging])

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto'
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 120)}px`
    }
  }, [input])

  const handleDragStart = (e: React.MouseEvent) => {
    const target = e.target as HTMLElement
    if (target.tagName === 'TEXTAREA' || target.tagName === 'BUTTON' || target.tagName === 'INPUT' ||
      target.closest('button') || target.closest('textarea') || target.closest('input')) return
    e.preventDefault()
    setIsDragging(true)
    dragStartRef.current = { mouseX: e.clientX, mouseY: e.clientY, posX: position.x, posY: position.y }
  }

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (isDragging && dragStartRef.current) {
        setPosition({
          x: dragStartRef.current.posX + (e.clientX - dragStartRef.current.mouseX),
          y: dragStartRef.current.posY + (e.clientY - dragStartRef.current.mouseY),
        })
      }
    }
    const handleMouseUp = () => { if (isDragging) { setIsDragging(false); dragStartRef.current = null } }
    if (isDragging) {
      document.addEventListener('mousemove', handleMouseMove)
      document.addEventListener('mouseup', handleMouseUp)
    }
    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
    }
  }, [isDragging])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!input.trim() || isLoading) return
    sendMessage({ text: input })
    setInput('')
    setAttachments([])
    setIsExpanded(true)
  }

  const handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files) return
    setAttachments(prev => [...prev, ...Array.from(files).map(file => ({
      id: crypto.randomUUID(), name: file.name, size: file.size, type: file.type,
    }))])
    setIsActive(true)
  }

  const formatFileSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  }

  return (
    <div
      ref={containerRef}
      onMouseDown={handleDragStart}
      style={{ transform: `translate(${position.x}px, ${position.y}px)` }}
      className={cn(
        'w-full max-w-2xl rounded-3xl overflow-hidden select-none',
        'transition-all duration-500 ease-out',
        isDragging ? 'cursor-grabbing transition-none' : 'cursor-grab',
        isActive
          ? 'bg-[var(--surface-raised)] shadow-2xl shadow-black/30 ring-1 ring-white/10'
          : 'bg-[var(--surface)] ring-1 ring-white/[0.06] hover:ring-white/[0.10]',
        className
      )}
    >
      <div
        style={{ backgroundColor: isActive ? undefined : `rgba(255,255,255,${idleOpacity})` }}
        className={cn('rounded-3xl transition-all duration-500', isActive && 'bg-transparent')}
      >
        {hasMessages && (
          <div className={cn(
            'overflow-hidden transition-all duration-500 ease-out',
            isExpanded ? 'max-h-[400px] opacity-100' : 'max-h-0 opacity-0'
          )}>
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              className="w-full flex items-center justify-center gap-2 py-2 text-white/30 hover:text-white/50 transition-colors border-b border-white/5"
            >
              <ChevronDown className="w-4 h-4" />
              <span className="text-xs">Hide messages</span>
            </button>
            <div className="max-h-80 overflow-y-auto p-4 space-y-4">
              {messages.map((message) => (
                <div key={message.id} className={cn('flex', message.role === 'user' ? 'justify-end' : 'justify-start')}>
                  <div className={cn(
                    'max-w-[80%] rounded-2xl px-4 py-2.5 text-sm leading-relaxed',
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
                  <div className="bg-white/5 rounded-2xl px-4 py-3 ring-1 ring-white/10">
                    <div className="flex gap-1">
                      <span className="w-1.5 h-1.5 bg-primary/60 rounded-full animate-bounce [animation-delay:-0.3s]" />
                      <span className="w-1.5 h-1.5 bg-primary/60 rounded-full animate-bounce [animation-delay:-0.15s]" />
                      <span className="w-1.5 h-1.5 bg-primary/60 rounded-full animate-bounce" />
                    </div>
                  </div>
                </div>
              )}
              {pendingPermission && (
                <PermissionCard
                  req={pendingPermission}
                  onApprove={() => approve(pendingPermission.id)}
                  onDeny={() => deny(pendingPermission.id)}
                  onSwitch={handleSwitch}
                />
              )}
              <div ref={messagesEndRef} />
            </div>
          </div>
        )}

        {hasMessages && !isExpanded && (
          <button
            onClick={() => setIsExpanded(true)}
            className="w-full flex items-center justify-center gap-2 py-2 text-white/30 hover:text-white/50 transition-colors border-b border-white/5"
          >
            <ChevronUp className="w-4 h-4" />
            <span className="text-xs">{messages.length} messages</span>
          </button>
        )}

        {attachments.length > 0 && (
          <div className="px-4 py-3 flex flex-wrap gap-2 border-b border-white/5">
            {attachments.map((file) => (
              <div key={file.id} className="flex items-center gap-2 bg-white/5 rounded-xl px-3 py-2 text-xs ring-1 ring-white/10">
                <Paperclip className="w-3.5 h-3.5 text-muted-foreground" />
                <span className="text-foreground truncate max-w-28">{file.name}</span>
                <span className="text-muted-foreground">{formatFileSize(file.size)}</span>
                <button onClick={() => setAttachments(prev => prev.filter(a => a.id !== file.id))} className="p-0.5 hover:bg-white/10 rounded-full transition-colors">
                  <X className="w-3 h-3 text-muted-foreground" />
                </button>
              </div>
            ))}
          </div>
        )}

        <form onSubmit={handleSubmit} className="p-3">
          <div className="flex items-end gap-2">
            <button type="button" onClick={() => fileInputRef.current?.click()}
              className={cn('p-2.5 rounded-xl transition-all duration-200',
                isActive ? 'text-white/50 hover:text-white/80 hover:bg-white/5' : 'text-white/20 hover:text-white/40'
              )} aria-label="Attach file">
              <Paperclip className="w-5 h-5" />
            </button>
            <input ref={fileInputRef} type="file" multiple onChange={handleFileSelect} className="hidden" />

            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onFocus={() => setIsActive(true)}
              onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSubmit(e) } }}
              placeholder={placeholder}
              disabled={isLoading}
              rows={1}
              className="flex-1 bg-transparent border-none outline-none resize-none py-2.5 text-sm placeholder:text-white/20 text-foreground disabled:opacity-50 cursor-text"
            />

            <button type="button" onClick={() => setIsRecording(!isRecording)}
              className={cn('p-2.5 rounded-xl transition-all duration-200',
                isRecording ? 'bg-red-500/20 text-red-400'
                  : isActive ? 'text-white/50 hover:text-white/80 hover:bg-white/5' : 'text-white/20 hover:text-white/40'
              )} aria-label={isRecording ? 'Stop recording' : 'Start voice input'}>
              {isRecording ? <MicOff className="w-5 h-5" /> : <Mic className="w-5 h-5" />}
            </button>

            <button type="submit" disabled={!input.trim() || isLoading}
              className={cn('p-2.5 rounded-xl transition-all duration-200',
                input.trim() && !isLoading
                  ? 'bg-primary text-primary-foreground hover:brightness-110'
                  : 'bg-white/5 text-white/15 cursor-not-allowed'
              )} aria-label="Send message">
              <ArrowUp className="w-5 h-5" />
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
