import React, { useState, useRef, useEffect } from "react";
import {
  Search,
  Send,
  MessageSquare,
  Sparkles,
  X,
  ArrowUpRight,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

interface FloatingBarProps {
  mode: "search" | "chat";
  // Search state
  searchQuery: string;
  onSearchChange: (query: string) => void;
  // Chat state & handlers
  chatHistory: ChatMessage[];
  onSendChatMessage: (message: string) => void;
  isSendingChat: boolean;
  onClearChat?: () => void;
}

export const FloatingBar: React.FC<FloatingBarProps> = ({
  mode,
  searchQuery,
  onSearchChange,
  chatHistory,
  onSendChatMessage,
  isSendingChat,
  onClearChat,
}) => {
  const { t } = useTranslation();
  const [chatInput, setChatInput] = useState("");
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const chatEndRef = useRef<HTMLDivElement>(null);

  // Auto-scroll chat history
  useEffect(() => {
    if (chatEndRef.current) {
      chatEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [chatHistory, isHistoryOpen]);

  // Open chat history if new message arrives
  useEffect(() => {
    if (chatHistory.length > 0) {
      setIsHistoryOpen(true);
    }
  }, [chatHistory.length]);

  const handleSend = () => {
    if (!chatInput.trim() || isSendingChat) return;
    onSendChatMessage(chatInput);
    setChatInput("");
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      if (mode === "chat") {
        handleSend();
      }
    } else if (e.key === "Escape") {
      if (mode === "search") {
        onSearchChange("");
      }
    }
  };

  return (
    <div className="fixed bottom-6 left-1/2 -translate-x-1/2 w-full max-w-xl z-40 px-4 flex flex-col items-center">
      {/* Floating Chat History Panel */}
      <AnimatePresence>
        {mode === "chat" && isHistoryOpen && chatHistory.length > 0 && (
          <motion.div
            initial={{ opacity: 0, y: 15, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 15, scale: 0.95 }}
            transition={{ type: "spring", stiffness: 300, damping: 25 }}
            className="w-full mb-3 bg-orange-off-white/95 border border-stone-mist/80 rounded-2xl shadow-2xl backdrop-blur-md overflow-hidden flex flex-col max-h-[320px]"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-2.5 border-b border-stone-mist/60 bg-warm-bone/40">
              <div className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-bark-grey">
                <Sparkles className="w-3.5 h-3.5 text-forest-green" />
                <span>{t("settings.meetings.chatWithMeeting")}</span>
              </div>
              <div className="flex items-center gap-2">
                {onClearChat && (
                  <button
                    onClick={onClearChat}
                    className="text-[10px] uppercase font-bold text-pebble hover:text-alarm-red transition-colors cursor-pointer"
                  >
                    {t("settings.meetings.clear")}
                  </button>
                )}
                <button
                  onClick={() => setIsHistoryOpen(false)}
                  className="text-bark-grey hover:text-charcoal p-0.5 rounded transition-colors"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>

            {/* Messages List */}
            <div className="flex-1 overflow-y-auto px-4 py-3 space-y-3 scrollbar-thin select-text">
              {chatHistory.map((msg, index) => (
                <div
                  key={index}
                  className={`flex flex-col ${
                    msg.role === "user" ? "items-end" : "items-start"
                  }`}
                >
                  <div
                    className={`max-w-[85%] rounded-xl px-3 py-2 text-sm ${
                      msg.role === "user"
                        ? "bg-forest-green text-orange-off-white font-medium"
                        : "bg-warm-bone/60 border border-stone-mist/40 text-charcoal markdown-answer"
                    }`}
                  >
                    {msg.role === "user" ? (
                      <p className="whitespace-pre-wrap">{msg.content}</p>
                    ) : (
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>
                        {msg.content}
                      </ReactMarkdown>
                    )}
                  </div>
                </div>
              ))}
              {isSendingChat && (
                <div className="flex justify-start">
                  <div className="bg-warm-bone/60 border border-stone-mist/40 rounded-xl px-3 py-2 text-sm flex items-center gap-2">
                    <span className="w-3.5 h-3.5 border-2 border-forest-green border-t-transparent rounded-full animate-spin"></span>
                    <span className="text-xs text-bark-grey italic">
                      {t("settings.meetings.asking")}
                    </span>
                  </div>
                </div>
              )}
              <div ref={chatEndRef} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* The Floating Pill Bar */}
      <motion.div
        layout
        transition={{ type: "spring", stiffness: 350, damping: 30 }}
        className="w-full bg-orange-off-white/90 border border-stone-mist/80 shadow-2xl rounded-2xl p-1.5 flex items-center gap-2 backdrop-blur-md relative group hover:border-stone-mist transition-colors"
      >
        <AnimatePresence mode="wait">
          {mode === "search" ? (
            <motion.div
              key="search-mode"
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
              transition={{ duration: 0.15 }}
              className="flex items-center gap-2 flex-1 px-2.5 py-1.5"
            >
              <Search className="w-4 h-4 text-bark-grey" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => onSearchChange(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("settings.meetings.searchPlaceholder")}
                className="flex-1 bg-transparent border-none text-charcoal placeholder-pebble text-sm focus:outline-none focus:ring-0"
                autoFocus
              />
              {searchQuery && (
                <button
                  onClick={() => onSearchChange("")}
                  className="text-bark-grey hover:text-charcoal p-1 transition-colors"
                >
                  <X className="w-4 h-4" />
                </button>
              )}
            </motion.div>
          ) : (
            <motion.div
              key="chat-mode"
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
              transition={{ duration: 0.15 }}
              className="flex items-center gap-2 flex-1 px-1 py-1"
            >
              {chatHistory.length > 0 && (
                <button
                  type="button"
                  onClick={() => setIsHistoryOpen(!isHistoryOpen)}
                  className={`p-2 rounded-xl transition-colors cursor-pointer ${
                    isHistoryOpen
                      ? "bg-forest-green/10 text-forest-green"
                      : "text-bark-grey hover:text-charcoal hover:bg-warm-bone/40"
                  }`}
                  title="Toggle Chat History"
                >
                  <MessageSquare className="w-4 h-4" />
                </button>
              )}
              <div className="flex items-center gap-2 flex-1 px-1.5 py-0.5">
                <Sparkles className="w-4 h-4 text-forest-green" />
                <input
                  type="text"
                  value={chatInput}
                  onChange={(e) => setChatInput(e.target.value)}
                  onKeyDown={handleKeyDown}
                  disabled={isSendingChat}
                  placeholder={t("settings.meetings.chatPlaceholder_detail")}
                  className="flex-1 bg-transparent border-none text-charcoal placeholder-pebble text-sm focus:outline-none focus:ring-0"
                  autoFocus
                />
              </div>
              <button
                type="button"
                onClick={handleSend}
                disabled={isSendingChat || !chatInput.trim()}
                className="p-2 rounded-xl bg-forest-green hover:bg-deep-forest-green disabled:opacity-40 disabled:cursor-not-allowed text-orange-off-white transition-all duration-150 active:scale-95 flex items-center justify-center cursor-pointer shadow-md"
              >
                {isSendingChat ? (
                  <span className="w-4 h-4 border-2 border-orange-off-white border-t-transparent rounded-full animate-spin"></span>
                ) : (
                  <Send className="w-4 h-4" />
                )}
              </button>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
};
