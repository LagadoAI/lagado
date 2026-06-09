import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { useState } from 'react';
import { TooltipProvider } from '@/components/ui/tooltip';
import LoginPage from './pages/LoginPage';
import FirstLaunchWelcome from './pages/FirstLaunchWelcome';
import FirstLaunchSystemDetected from './pages/FirstLaunchSystemDetected';
import FirstLaunchModelSelection from './pages/FirstLaunchModelSelection';
import FirstLaunchPermissionsSetup from './pages/FirstLaunchPermissionsSetup';
import ChatDefault from './pages/ChatDefault';
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
import ServerManagement from './pages/ServerManagement';
import VMManager from './pages/VMManager';
import './index.css';
import { ChatProvider } from './hooks/use-chat-context';

function AppRoutes({ isLoggedIn, setIsLoggedIn }: { isLoggedIn: boolean; setIsLoggedIn: (v: boolean) => void }) {
  if (!isLoggedIn) {
    return <LoginPage onLogin={() => setIsLoggedIn(true)} />;
  }

  return (
    <ChatProvider>
      <Routes>
        <Route path="/setup/welcome" element={<FirstLaunchWelcome onNext={() => {}} />} />
        <Route path="/setup/system" element={<FirstLaunchSystemDetected onNext={() => {}} />} />
        <Route path="/setup/models" element={<FirstLaunchModelSelection onNext={() => {}} />} />
        <Route path="/setup/permissions" element={<FirstLaunchPermissionsSetup onComplete={() => {}} />} />
        <Route path="/chat" element={<ChatDefault />} />
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
        <Route path="/server" element={<ServerManagement />} />
        <Route path="/vm" element={<VMManager />} />
        <Route path="/" element={<ChatDefault />} />
      </Routes>
    </ChatProvider>
  );
}

export default function App() {
  const [isLoggedIn, setIsLoggedIn] = useState(false);
  return (
    <BrowserRouter>
      <TooltipProvider>
        <AppRoutes isLoggedIn={isLoggedIn} setIsLoggedIn={setIsLoggedIn} />
      </TooltipProvider>
    </BrowserRouter>
  );
}
