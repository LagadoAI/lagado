import { useNavigate } from "react-router-dom";
import { ChatBox } from "@/components/ui/chat-box";
import { SidePane } from "@/components/ui/side-pane";

export default function ImmersiveDefault() {
  const navigate = useNavigate();

  return (
    <div className="min-h-screen bg-black relative overflow-hidden">
      <div className="w-full h-screen bg-transparent" />

      <ChatBox className="fixed bottom-8 left-1/2 -translate-x-1/2 z-40" />

      <SidePane>
        <button
          onClick={() => navigate("/chat")}
          className="w-full text-left px-4 py-2 rounded-xl text-sm text-white/70 bg-white/5 hover:bg-white/10 ring-1 ring-white/10 transition-colors"
        >
          ← Return to Chat
        </button>
      </SidePane>
    </div>
  );
}
