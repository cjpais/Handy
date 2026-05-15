import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

type AgentSessionStatus = "idle" | "running";

interface AgentSessionSnapshot {
  status: AgentSessionStatus;
}

interface RealtimeEvent {
  type?: string;
  response?: {
    output?: Array<{
      type?: string;
      name?: string;
      call_id?: string;
      arguments?: string;
    }>;
  };
  call_id?: string;
  name?: string;
  arguments?: string;
}

const CHECK_CALENDAR_TOOL = {
  type: "function",
  name: "check_calendar",
  description:
    "Checks Google Calendar availability for a requested date and optional local time.",
  parameters: {
    type: "object",
    properties: {
      date: {
        type: "string",
        description: "The requested date in YYYY-MM-DD format.",
      },
      time: {
        type: "string",
        description: "The requested local time in HH:MM 24-hour format.",
      },
      durationMinutes: {
        type: "number",
        description: "Meeting duration in minutes.",
      },
    },
    required: ["date"],
    additionalProperties: false,
  },
};

const LIST_CALENDAR_EVENTS_TOOL = {
  type: "function",
  name: "list_calendar_events",
  description:
    "Lists Google Calendar events for a date. Use this for agenda questions like what is on my calendar today.",
  parameters: {
    type: "object",
    properties: {
      date: {
        type: "string",
        description: "The date to list in YYYY-MM-DD format.",
      },
    },
    required: ["date"],
    additionalProperties: false,
  },
};

const GMAIL_SEARCH_TOOL = {
  type: "function",
  name: "gmail_search",
  description: "Searches connected Gmail messages with Gmail query syntax.",
  parameters: {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "A Gmail search query, such as from:alice newer_than:7d.",
      },
      maxResults: {
        type: "number",
        description: "Maximum number of messages to return.",
      },
    },
    required: ["query"],
    additionalProperties: false,
  },
};

const GMAIL_CREATE_DRAFT_TOOL = {
  type: "function",
  name: "gmail_create_draft",
  description:
    "Creates a Gmail draft. This tool never sends email; it only saves a draft for the user to review.",
  parameters: {
    type: "object",
    properties: {
      to: {
        type: "string",
        description: "Recipient email address.",
      },
      subject: {
        type: "string",
        description: "Draft subject line.",
      },
      body: {
        type: "string",
        description: "Plain text draft body.",
      },
      cc: {
        type: "string",
        description: "Optional comma-separated CC recipients.",
      },
    },
    required: ["to", "subject", "body"],
    additionalProperties: false,
  },
};

const NOTION_SEARCH_TOOL = {
  type: "function",
  name: "notion_search",
  description: "Searches the connected Notion workspace.",
  parameters: {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "The Notion search query.",
      },
    },
    required: ["query"],
    additionalProperties: false,
  },
};

const NOTION_SEARCH_TASKS_TOOL = {
  type: "function",
  name: "notion_search_tasks",
  description:
    "Searches the configured Notion Tasks database only. Use this for task, to-do, follow-up, and action item lookup.",
  parameters: {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "The task search query.",
      },
      ownerName: {
        type: "string",
        description:
          'Optional task owner to filter by. Omit this to search only the local user\'s tasks. Use "all" only when the user explicitly asks for all owners.',
      },
    },
    required: ["query"],
    additionalProperties: false,
  },
};

const PROPOSE_NOTION_LEAD_TOOL = {
  type: "function",
  name: "propose_notion_lead",
  description:
    "Drafts a Notion lead for user approval in the floating review overlay. This does not create anything until the user approves it.",
  parameters: {
    type: "object",
    properties: {
      company: {
        type: "string",
        description: "Lead company or organization name.",
      },
      contactName: {
        type: "string",
        description: "Primary contact name.",
      },
      role: {
        type: "string",
        description: "Primary contact role or title.",
      },
      email: {
        type: "string",
        description: "Primary contact email address.",
      },
      phone: {
        type: "string",
        description: "Primary contact phone number.",
      },
      source: {
        type: "string",
        description: "Where the lead came from.",
      },
      status: {
        type: "string",
        description: "Current lead status.",
      },
      notes: {
        type: "string",
        description: "Useful notes, context, or qualification details.",
      },
      nextStep: {
        type: "string",
        description: "Suggested next step.",
      },
    },
    required: [],
    additionalProperties: false,
  },
};

const PROPOSE_NOTION_DEAL_TOOL = {
  type: "function",
  name: "propose_notion_deal",
  description:
    "Drafts a Notion deal for user approval in the floating review overlay. This does not create anything until the user approves it.",
  parameters: {
    type: "object",
    properties: {
      dealName: {
        type: "string",
        description: "Deal name or short opportunity title.",
      },
      company: {
        type: "string",
        description: "Company or account name for the deal.",
      },
      contactName: {
        type: "string",
        description: "Primary contact name.",
      },
      amount: {
        type: "string",
        description: "Deal value, including currency if known.",
      },
      stage: {
        type: "string",
        description: "Current sales stage.",
      },
      closeDate: {
        type: "string",
        description: "Expected close date in YYYY-MM-DD format.",
      },
      source: {
        type: "string",
        description: "Where the deal came from.",
      },
      nextStep: {
        type: "string",
        description: "Suggested next step.",
      },
      notes: {
        type: "string",
        description: "Useful context, qualification details, or risks.",
      },
    },
    required: [],
    additionalProperties: false,
  },
};

const PROPOSE_NOTION_TASK_TOOL = {
  type: "function",
  name: "propose_notion_task",
  description:
    "Drafts a Notion task for user approval in the floating review overlay. This does not create anything until the user approves it.",
  parameters: {
    type: "object",
    properties: {
      taskName: {
        type: "string",
        description: "Short task title.",
      },
      ownerName: {
        type: "string",
        description: "Person responsible for the task, if known.",
      },
      dueDate: {
        type: "string",
        description: "Due date in YYYY-MM-DD format, if known.",
      },
      priority: {
        type: "string",
        description: "Priority, if stated.",
      },
      status: {
        type: "string",
        description: "Current task status, if stated.",
      },
      relatedCompany: {
        type: "string",
        description: "Related company, account, or client, if known.",
      },
      relatedContact: {
        type: "string",
        description: "Related person or contact, if known.",
      },
      notes: {
        type: "string",
        description: "Useful details or context for the task.",
      },
    },
    required: ["taskName"],
    additionalProperties: false,
  },
};

const GRANOLA_SEARCH_TOOL = {
  type: "function",
  name: "granola_search_notes",
  description:
    "Searches connected Granola meeting notes by title, date, participants, and note content. Can optionally include full transcripts for exact wording or detailed reconstruction.",
  parameters: {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "The meeting-note search query.",
      },
      includeTranscript: {
        type: "boolean",
        description:
          "Set true when the user asks for exact wording, quotes, detailed reconstruction, or the full transcript.",
      },
      maxTranscripts: {
        type: "integer",
        minimum: 1,
        maximum: 3,
        description:
          "Maximum number of matched meetings to fetch transcripts for. Use 1 unless the user asks across multiple meetings.",
      },
    },
    required: ["query"],
    additionalProperties: false,
  },
};

const AGENT_TOOL_NAMES = new Set([
  "check_calendar",
  "list_calendar_events",
  "gmail_search",
  "gmail_create_draft",
  "notion_search",
  "notion_search_tasks",
  "propose_notion_lead",
  "propose_notion_deal",
  "propose_notion_task",
  "granola_search_notes",
]);

function localDateIso(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function buildSessionUpdateEvent() {
  const today = localDateIso();
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;

  return {
    type: "session.update",
    session: {
      type: "realtime",
      instructions: `You are Samantha, a concise local desktop voice agent. Speak naturally and briefly. Today's local date is ${today}, and the local timezone is ${timeZone}. Resolve relative dates like today and tomorrow before calling tools. Use list_calendar_events for agenda questions like "what is on my calendar today"; use check_calendar only for availability questions about a specific time. You can use connected local tools for Notion, Granola, Gmail, and Google Calendar. For Granola, use granola_search_notes to find meetings; set includeTranscript to true when the user asks for exact wording, quotes, detailed reconstruction, or a full transcript. For Gmail, you may search email and create drafts, but you must never claim to send email. If asked to send email, create a draft and tell the user it is ready for review. Use notion_search_tasks for task, to-do, follow-up, and action item lookup. When searching tasks, omit ownerName unless the user names a different owner; the backend defaults omitted ownerName to the local user. Pass ownerName only when the user asks for another person's tasks, and pass ownerName "all" only when the user explicitly asks for all owners. To add a Notion lead, collect the details the user provides, ask only for clearly missing essentials, then call propose_notion_lead so the user can approve it in the floating overlay. To add a Notion deal or opportunity, collect the deal details the user provides, ask only for clearly missing essentials, then call propose_notion_deal so the user can approve it in the floating overlay. To add a Notion task, collect a short task title plus any owner, due date, priority, status, related company/contact, and notes the user provides, then call propose_notion_task so the user can approve it in the floating overlay. Never claim a Notion lead, deal, or task was created until the user approves it. If a tool is not connected, say which connection is needed.`,
      output_modalities: ["audio"],
      audio: {
        output: {
          voice: "marin",
        },
      },
      tools: [
        CHECK_CALENDAR_TOOL,
        LIST_CALENDAR_EVENTS_TOOL,
        GMAIL_SEARCH_TOOL,
        GMAIL_CREATE_DRAFT_TOOL,
        NOTION_SEARCH_TOOL,
        NOTION_SEARCH_TASKS_TOOL,
        PROPOSE_NOTION_LEAD_TOOL,
        PROPOSE_NOTION_DEAL_TOOL,
        PROPOSE_NOTION_TASK_TOOL,
        GRANOLA_SEARCH_TOOL,
      ],
      tool_choice: "auto",
    },
  };
}

function parseArguments(rawArguments: string | undefined) {
  if (!rawArguments) return {};

  try {
    return JSON.parse(rawArguments) as Record<string, unknown>;
  } catch {
    return {};
  }
}

function sendRealtimeEvent(channel: RTCDataChannel, event: unknown) {
  if (channel.readyState === "open") {
    channel.send(JSON.stringify(event));
  }
}

function logRuntimeEvent(message: string) {
  invoke("log_agent_runtime_event", { message }).catch(console.warn);
}

export const AgentRuntime = () => {
  const { t } = useTranslation();
  const peerConnectionRef = useRef<RTCPeerConnection | null>(null);
  const dataChannelRef = useRef<RTCDataChannel | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const audioElementRef = useRef<HTMLAudioElement | null>(null);
  const handledCallIdsRef = useRef<Set<string>>(new Set());
  const isConnectingRef = useRef(false);

  const disconnectRealtime = useCallback(() => {
    dataChannelRef.current?.close();
    dataChannelRef.current = null;

    peerConnectionRef.current?.close();
    peerConnectionRef.current = null;

    mediaStreamRef.current?.getTracks().forEach((track) => track.stop());
    mediaStreamRef.current = null;

    if (audioElementRef.current) {
      audioElementRef.current.srcObject = null;
      audioElementRef.current.remove();
      audioElementRef.current = null;
    }

    handledCallIdsRef.current.clear();
    isConnectingRef.current = false;
  }, []);

  const handleFunctionCall = useCallback(async (event: RealtimeEvent) => {
    if (
      event.type === "response.function_call_arguments.done" ||
      event.type === "response.done"
    ) {
      logRuntimeEvent(`received realtime event: ${event.type}`);
    }

    const calls =
      event.type === "response.done"
        ? event.response?.output?.filter(
            (item) => item.type === "function_call",
          )
        : event.type === "response.function_call_arguments.done"
          ? [
              {
                type: "function_call",
                name: event.name,
                call_id: event.call_id,
                arguments: event.arguments,
              },
            ]
          : [];

    for (const call of calls ?? []) {
      if (
        !call.name ||
        !call.call_id ||
        handledCallIdsRef.current.has(call.call_id) ||
        !AGENT_TOOL_NAMES.has(call.name)
      ) {
        continue;
      }

      handledCallIdsRef.current.add(call.call_id);
      const args = parseArguments(call.arguments);
      logRuntimeEvent(
        `tool call started: ${call.name} ${JSON.stringify(args)}`,
      );
      const backendToolName =
        call.name === "check_calendar"
          ? "calendar_check_availability"
          : call.name === "list_calendar_events"
            ? "calendar_list_events"
            : call.name === "gmail_create_draft"
              ? "gmail_create_draft"
              : call.name;
      let result: unknown;
      try {
        if (
          call.name === "propose_notion_lead" ||
          call.name === "propose_notion_deal" ||
          call.name === "propose_notion_task"
        ) {
          const review = await invoke(call.name, {
            argumentsJson: JSON.stringify(args),
          });
          result = {
            status: "pending_review",
            review,
          };
        } else {
          const output = await invoke<string>("run_agent_connection_tool", {
            name: backendToolName,
            argumentsJson: JSON.stringify(args),
          });
          result = JSON.parse(output) as unknown;
        }
        logRuntimeEvent(`tool call succeeded: ${call.name}`);
      } catch (error) {
        result = {
          error: String(error),
          tool: backendToolName,
        };
        logRuntimeEvent(`tool call failed: ${call.name}: ${String(error)}`);
      }
      const channel = dataChannelRef.current;

      if (!channel) {
        logRuntimeEvent(
          `tool call output dropped, data channel closed: ${call.name}`,
        );
        return;
      }

      sendRealtimeEvent(channel, {
        type: "conversation.item.create",
        item: {
          type: "function_call_output",
          call_id: call.call_id,
          output: JSON.stringify(result),
        },
      });
      sendRealtimeEvent(channel, { type: "response.create" });
      logRuntimeEvent(`tool call output sent: ${call.name}`);
    }
  }, []);

  const connectRealtime = useCallback(async () => {
    if (peerConnectionRef.current || isConnectingRef.current) return;

    isConnectingRef.current = true;

    try {
      const peerConnection = new RTCPeerConnection();
      peerConnectionRef.current = peerConnection;

      const audioElement = document.createElement("audio");
      audioElement.autoplay = true;
      audioElementRef.current = audioElement;
      document.body.appendChild(audioElement);

      peerConnection.ontrack = (event) => {
        audioElement.srcObject = event.streams[0];
      };

      const mediaStream = await navigator.mediaDevices.getUserMedia({
        audio: true,
      });
      mediaStreamRef.current = mediaStream;
      peerConnection.addTrack(mediaStream.getTracks()[0], mediaStream);

      const dataChannel = peerConnection.createDataChannel("oai-events");
      dataChannelRef.current = dataChannel;

      dataChannel.addEventListener("open", () => {
        logRuntimeEvent("data channel open");
        sendRealtimeEvent(dataChannel, buildSessionUpdateEvent());
        sendRealtimeEvent(dataChannel, {
          type: "conversation.item.create",
          item: {
            type: "message",
            role: "user",
            content: [
              {
                type: "input_text",
                text: "Start this voice session. Briefly introduce yourself as Samantha and say you are listening.",
              },
            ],
          },
        });
        sendRealtimeEvent(dataChannel, { type: "response.create" });
      });

      dataChannel.addEventListener("message", (message) => {
        try {
          const event = JSON.parse(message.data) as RealtimeEvent;
          if (
            event.type?.startsWith("error") ||
            event.type === "response.function_call_arguments.done"
          ) {
            logRuntimeEvent(`realtime event: ${event.type}`);
          }
          void handleFunctionCall(event);
        } catch (error) {
          console.warn("Failed to handle realtime event:", error);
          logRuntimeEvent(`failed to parse realtime event: ${String(error)}`);
        }
      });

      const offer = await peerConnection.createOffer();
      await peerConnection.setLocalDescription(offer);

      if (!offer.sdp) {
        throw new Error("WebRTC offer did not include SDP");
      }

      const answerSdp = await invoke<string>("create_agent_realtime_call", {
        sdp: offer.sdp,
      });

      await peerConnection.setRemoteDescription({
        type: "answer",
        sdp: answerSdp,
      });
    } catch (error) {
      disconnectRealtime();
      toast.error(t("settings.agent.realtime.errorTitle"), {
        description: String(error),
      });
      await invoke("stop_agent_session").catch(console.error);
    } finally {
      isConnectingRef.current = false;
    }
  }, [disconnectRealtime, handleFunctionCall, t]);

  useEffect(() => {
    invoke<AgentSessionSnapshot>("get_agent_session")
      .then((session) => {
        if (session.status === "running") {
          connectRealtime();
        }
      })
      .catch(console.warn);

    const unlisten = listen<AgentSessionSnapshot>(
      "agent-session-changed",
      (event) => {
        if (event.payload.status === "running") {
          connectRealtime();
        } else {
          disconnectRealtime();
        }
      },
    );

    return () => {
      disconnectRealtime();
      unlisten.then((fn) => fn());
    };
  }, [connectRealtime, disconnectRealtime]);

  return null;
};
