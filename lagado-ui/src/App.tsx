import { BrowserRouter, Routes, Route, useNavigate } from 'react-router-dom';
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { TooltipProvider } from '@/components/ui/tooltip';
import Awakening from './pages/Awakening';
import LoginPage from './pages/LoginPage';
import SignupPage from './pages/SignupPage';
import FirstLaunchWelcome from './pages/FirstLaunchWelcome';
import FirstLaunchSystemDetected from './pages/FirstLaunchSystemDetected';
import FirstLaunchModelSelection from './pages/FirstLaunchModelSelection';
import FirstLaunchPermissionsSetup from './pages/FirstLaunchPermissionsSetup';
import ChatDefault from './pages/ChatDefault';
import DesignSystem from './pages/DesignSystem';
import ImmersiveDefault from './pages/ImmersiveDefault';
import ImmersiveTyping from './pages/ImmersiveTyping';
import ImmersiveAgentRunning from './pages/ImmersiveAgentRunning';
import ImmersiveAgentPaused from './pages/ImmersiveAgentPaused';
import ImmersiveWithSidebar from './pages/ImmersiveWithSidebar';
import CodePage from './pages/CodePage';
import CodeWithSandboxOutput from './pages/CodeWithSandboxOutput';
import CodeWithTerminal from './pages/CodeWithTerminal';
import VaultDefault from './pages/VaultDefault';
import VaultFilePreview from './pages/VaultFilePreview';
import VaultStorageWarning from './pages/VaultStorageWarning';
import TerminalDefault from './pages/TerminalDefault';
import TerminalMultipleTabs from './pages/TerminalMultipleTabs';
import TerminalAgentRunning from './pages/TerminalAgentRunning';
import SettingsMain from './pages/SettingsMain';
import MCPManager from './pages/MCPManager';
import MCPAddTool from './pages/MCPAddTool';
import ServerManagement from './pages/ServerManagement';
import VMManager from './pages/VMManager';
import './index.css';
import { ChatProvider } from './hooks/use-chat-context';

type AuthState = 'loading' | 'awakening' | 'signup' | 'login' | 'app'

function AppRoutes() {
  const navigate = useNavigate()
  const [authState, setAuthState] = useState<AuthState>('loading')

  useEffect(() => {
    const hasAwakened = localStorage.getItem('lagado_awakened') === 'true'
    if (!hasAwakened) {
      setAuthState('awakening')
      return
    }
    invoke<{ needs_setup: boolean; locked: boolean }>('auth_check')
      .then(info => {
        setAuthState(info.needs_setup ? 'signup' : 'login')
      })
      .catch(() => setAuthState('login'))
  }, [])

  if (authState === 'loading') {
    return <div className="min-h-screen bg-lagado-bg" />
  }

  if (authState === 'awakening') {
    return <Awakening />
  }

  if (authState === 'signup') {
    return (
      <Routes>
        <Route path="*" element={<SignupPage onSignup={() => setAuthState('app')} />} />
        <Route path="/setup/welcome" element={<FirstLaunchWelcome onNext={() => navigate('/setup/system')} />} />
        <Route path="/setup/system" element={<FirstLaunchSystemDetected onNext={() => navigate('/setup/models')} />} />
        <Route path="/setup/models" element={<FirstLaunchModelSelection onNext={() => navigate('/setup/permissions')} />} />
        <Route path="/setup/permissions" element={<FirstLaunchPermissionsSetup onComplete={() => { setAuthState('app'); navigate('/chat') }} />} />
      </Routes>
    )
  }

  if (authState === 'login') {
    return <LoginPage onLogin={() => setAuthState('app')} />
  }

  return (
    <ChatProvider>
      <Routes>
        <Route path="/awakening" element={<Awakening />} />
        <Route path="/setup/welcome" element={<FirstLaunchWelcome onNext={() => navigate('/setup/system')} />} />
        <Route path="/setup/system" element={<FirstLaunchSystemDetected onNext={() => navigate('/setup/models')} />} />
        <Route path="/setup/models" element={<FirstLaunchModelSelection onNext={() => navigate('/setup/permissions')} />} />
        <Route path="/setup/permissions" element={<FirstLaunchPermissionsSetup onComplete={() => navigate('/chat')} />} />
        <Route path="/chat" element={<ChatDefault />} />
        <Route path="/design" element={<DesignSystem />} />
        <Route path="/immersive" element={<ImmersiveDefault />} />
        <Route path="/immersive/typing" element={<ImmersiveTyping />} />
        <Route path="/immersive/running" element={<ImmersiveAgentRunning />} />
        <Route path="/immersive/paused" element={<ImmersiveAgentPaused />} />
        <Route path="/immersive/sidebar" element={<ImmersiveWithSidebar />} />
        <Route path="/code" element={<CodePage />} />
        <Route path="/code/sandbox" element={<CodeWithSandboxOutput />} />
        <Route path="/code/terminal" element={<CodeWithTerminal />} />
        <Route path="/vault" element={<VaultDefault />} />
        <Route path="/vault/preview" element={<VaultFilePreview />} />
        <Route path="/vault/warning" element={<VaultStorageWarning />} />
        <Route path="/terminal" element={<TerminalDefault />} />
        <Route path="/terminal/multi" element={<TerminalMultipleTabs />} />
        <Route path="/terminal/agent" element={<TerminalAgentRunning />} />
        <Route path="/settings" element={<SettingsMain />} />
        <Route path="/mcp" element={<MCPManager />} />
        <Route path="/mcp/add" element={<MCPAddTool />} />
        <Route path="/server" element={<ServerManagement />} />
        <Route path="/vm" element={<VMManager />} />
        <Route path="/" element={<ChatDefault />} />
      </Routes>
    </ChatProvider>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <TooltipProvider>
        <AppRoutes />
      </TooltipProvider>
    </BrowserRouter>
  )
}
