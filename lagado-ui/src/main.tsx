import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import AgentWindow from './AgentWindow'
import './index.css'

// The Agent OS window loads this same bundle with ?view=agent → render ONLY the bare VM surface.
// Everything else (the control surface, sidebar, auth) is the main window's App.
const isAgentWindow = new URLSearchParams(window.location.search).get('view') === 'agent'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    {isAgentWindow ? <AgentWindow /> : <App />}
  </React.StrictMode>,
)
