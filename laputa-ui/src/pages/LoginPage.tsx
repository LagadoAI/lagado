import React, { useState } from "react";
import { useNavigate } from "react-router-dom";

interface LoginPageProps {
  onLogin: () => void;
}

export default function LoginPage({ onLogin }: LoginPageProps) {
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleLogin = async () => {
    setError(null);
    if (!username.trim()) { setError("Username is required"); return; }
    if (!passphrase) { setError("Passphrase is required"); return; }

    setIsLoading(true);
    try {
      await new Promise((resolve) => setTimeout(resolve, 400));
      onLogin();
      navigate("/chat");
    } catch (err) {
      setError("Login failed.");
    } finally {
      setIsLoading(false);
    }
  };

  const handleCreateAccount = () => {
    onLogin();
    navigate("/setup/welcome");
  };

  return (
    <div className="min-h-screen bg-laputa-bg flex items-center justify-center px-4">
      <div className="w-full max-w-md">
        <div className="text-center mb-8">
          <div className="inline-block w-24 h-24 bg-laputa-surface-2 rounded-md mb-4 border border-laputa-border" />
           <h1 style={{ fontSize: "72px" }} className="text-laputa-text-bright font-bold tracking-wider mb-2">    
            LAPUTA
          </h1>
          <p className="text-body-sm text-laputa-text-dim">
            Local • Private • Yours
          </p>
        </div>

        <div className="bg-laputa-surface border border-laputa-border rounded-md p-6">
          {error && (
            <div className="mb-4 p-3 bg-laputa-red bg-opacity-10 border border-laputa-red rounded-md">
              <p className="text-body-sm text-laputa-red">{error}</p>
            </div>
          )}

          <div className="space-y-4">
            <div>
              <label className="block text-body-sm text-laputa-text-dim mb-2">
                Username
              </label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="Enter username"
                className="w-full px-3 py-2 bg-laputa-surface-2 border border-laputa-border rounded-md text-laputa-text placeholder-laputa-text-dim focus:border-laputa-red focus:outline-none"
              />
            </div>

            <div>
              <label className="block text-body-sm text-laputa-text-dim mb-2">
                Passphrase
              </label>
              <input
                type="password"
                value={passphrase}
                onChange={(e) => setPassphrase(e.target.value)}
                placeholder="Enter passphrase"
                onKeyPress={(e) => e.key === "Enter" && handleLogin()}
                className="w-full px-3 py-2 bg-laputa-surface-2 border border-laputa-border rounded-md text-laputa-text placeholder-laputa-text-dim focus:border-laputa-red focus:outline-none"
              />
            </div>

            <button
              onClick={handleLogin}
              disabled={isLoading}
              className="w-full px-4 py-2 bg-laputa-red text-white rounded-md font-semibold hover:bg-opacity-90 transition-colors disabled:opacity-50"
            >
              {isLoading ? "Logging in..." : "Login"}
            </button>
          </div>

          <div className="mt-6 pt-4 border-t border-laputa-border text-center">
            <button
              onClick={handleCreateAccount}
              className="text-caption text-laputa-purple hover:underline"
            >
              First time? Create account
            </button>
          </div>
        </div>

        <p className="text-center mt-6 text-caption text-laputa-text-dim">
          Encrypted with AES-256 | Local authentication only
        </p>
      </div>
    </div>
  );
}
