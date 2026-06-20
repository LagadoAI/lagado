import { useNavigate, useLocation } from 'react-router-dom'
import { MessageSquare, Monitor, Lock, Settings, Plus, Search } from 'lucide-react'
import { useChatContext } from '@/hooks/use-chat-context'

const NAV = [
  { id: 'chat',      label: 'Control',   Icon: MessageSquare, path: '/chat' },
  { id: 'agent',     label: 'Agent',     Icon: Monitor,       path: '/agent' },
  { id: 'vault',     label: 'Vault',     Icon: Lock,          path: '/vault' },
  { id: 'settings',  label: 'Settings',  Icon: Settings,      path: '/settings' },
]

export function AppSidebar() {
  const navigate = useNavigate()
  const location = useLocation()
  const { connState } = useChatContext()

  const username = localStorage.getItem('lagado_username') || 'local_user'
  const initials = username.slice(0, 2).toUpperCase()

  const pillClass = connState === 'connected'
    ? 'lg-pill lg-pill--connected'
    : connState === 'connecting'
    ? 'lg-pill lg-pill--connecting'
    : 'lg-pill lg-pill--disconnected'
  const pillLabel = connState === 'connected' ? 'Connected' : connState === 'connecting' ? 'Connecting' : 'Offline'

  return (
    <div style={{
      width: 256, flexShrink: 0,
      background: 'var(--glass-opaque)',
      borderRight: '1px solid var(--line-700)',
      display: 'flex', flexDirection: 'column',
    }}>
      {/* Logo lockup */}
      <div style={{ padding: '12px 12px 8px', display: 'flex', alignItems: 'center', gap: 9 }}>
        <img src="/lagado-mark.png" width={22} height={22} alt="" style={{ filter: 'drop-shadow(0 0 6px rgba(139,92,246,.4))' }} />
        <span style={{ fontFamily: 'var(--font-display)', fontWeight: 700, letterSpacing: '.14em', textTransform: 'uppercase', color: 'var(--text-strong)', fontSize: 15 }}>
          Lagado
        </span>
      </div>

      {/* New conversation */}
      <div style={{ padding: '0 12px 8px' }}>
        <button
          className="lg-btn lg-btn--primary lg-btn--md"
          style={{ width: '100%', gap: 6 }}
          onClick={() => navigate('/chat')}
        >
          <Plus size={14} />
          New conversation
        </button>
      </div>

      {/* Search */}
      <div style={{ padding: '0 12px 10px' }}>
        <div style={{ position: 'relative' }}>
          <Search style={{ position: 'absolute', left: 9, top: '50%', transform: 'translateY(-50%)', color: 'var(--text-dim)', pointerEvents: 'none' }} size={14} />
          <input className="lg-field" placeholder="Search chats…" style={{ paddingLeft: 30, height: 32 }} />
        </div>
      </div>

      {/* Nav */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '0 8px' }}>
        <div className="t-label" style={{ padding: '8px 8px 4px' }}>Recent</div>
        <div style={{ padding: '6px 8px', fontSize: 13, color: 'var(--text-dim)', fontStyle: 'italic' }}>
          No conversations yet
        </div>
        <div className="t-label" style={{ padding: '12px 8px 4px' }}>Surfaces</div>
        {NAV.map(({ id, label, Icon, path }) => {
          const isActive = location.pathname === path || (path === '/chat' && location.pathname === '/')
          return (
            <button key={id} className={`nav-item ${isActive ? 'nav-item--active' : ''}`} onClick={() => navigate(path)}>
              <Icon size={16} />
              {label}
            </button>
          )
        })}
      </div>

      {/* User footer */}
      <div style={{ borderTop: '1px solid var(--line-700)', padding: 12, display: 'flex', alignItems: 'center', gap: 10 }}>
        <div style={{
          width: 32, height: 32, borderRadius: '9999px',
          background: 'var(--surface-raised)', border: '1px solid var(--line-700)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 11, fontWeight: 700, color: 'var(--text-body)',
          fontFamily: 'var(--font-display)', flexShrink: 0,
        }}>
          {initials}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13, color: 'var(--text-strong)', fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {username}
          </div>
          <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>Sovereign</div>
        </div>
        <div className={pillClass} style={{ fontSize: 11, height: 22, padding: '0 8px', gap: 5, flexShrink: 0 }}>
          <span className="lg-pill__dot" />
          {pillLabel}
        </div>
      </div>
    </div>
  )
}
