import { BrowserRouter } from 'react-router-dom'
import { ChatProvider } from './hooks/use-chat-context'
import ImmersiveDefault from './pages/ImmersiveDefault'

// Root for the SEPARATE Agent OS window: ONLY the bare VM the agent operates, wrapped in the
// providers it needs (ChatProvider for the HITL approval overlay; Router for the close affordance).
// No sidebar, no auth gate — the window is spawned post-auth from the main control window.
export default function AgentWindow() {
  return (
    <ChatProvider>
      <BrowserRouter>
        <ImmersiveDefault />
      </BrowserRouter>
    </ChatProvider>
  )
}
