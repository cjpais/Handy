import React from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";

interface MarkdownContentProps {
  markdown: string;
}

const allowedElements = [
  "a",
  "blockquote",
  "br",
  "code",
  "del",
  "em",
  "h1",
  "h2",
  "h3",
  "hr",
  "img",
  "input",
  "li",
  "ol",
  "p",
  "pre",
  "strong",
  "table",
  "tbody",
  "td",
  "th",
  "thead",
  "tr",
  "ul",
];

const isSafeUrl = (url: string) => {
  try {
    const parsed = new URL(url);
    return ["http:", "https:", "mailto:"].includes(parsed.protocol);
  } catch {
    return false;
  }
};

const openSafeUrl = async (url: string) => {
  if (!isSafeUrl(url)) return;

  try {
    await openUrl(url);
  } catch (error) {
    console.error("Failed to open release note link:", error);
  }
};

const isSafeImageSrc = (src: string) => {
  if (!src.startsWith("/release-notes/")) return false;
  if (src.includes("\\") || src.includes("..")) return false;

  return true;
};

const components: Components = {
  h1: ({ children }) => (
    <h3 className="text-base font-semibold leading-snug text-text">
      {children}
    </h3>
  ),
  h2: ({ children }) => (
    <h3 className="text-[15px] font-semibold leading-snug text-text">
      {children}
    </h3>
  ),
  h3: ({ children }) => (
    <h3 className="text-sm font-semibold leading-snug text-text">{children}</h3>
  ),
  p: ({ children }) => (
    <p className="text-sm leading-relaxed text-text/80">{children}</p>
  ),
  ul: ({ children, className }) => {
    const isTaskList = className?.includes("contains-task-list");

    return (
      <ul
        className={`space-y-1 text-sm leading-relaxed text-text/80 ${
          isTaskList ? "ps-0" : "list-disc ps-5"
        }`}
      >
        {children}
      </ul>
    );
  },
  li: ({ children, className }) => {
    const isTaskListItem = className?.includes("task-list-item");

    return (
      <li
        className={`${isTaskListItem ? "list-none" : "pl-1"} marker:text-text/50`}
      >
        {children}
      </li>
    );
  },
  input: ({ checked, type }) => {
    if (type !== "checkbox") return null;

    return (
      <input
        type="checkbox"
        checked={Boolean(checked)}
        disabled
        readOnly
        className="me-2 h-3.5 w-3.5 align-middle accent-logo-primary"
      />
    );
  },
  ol: ({ children }) => (
    <ol className="list-decimal space-y-1 ps-5 text-sm leading-relaxed text-text/80">
      {children}
    </ol>
  ),
  del: ({ children }) => (
    <del className="text-text/60 line-through">{children}</del>
  ),
  br: () => <br />,
  hr: () => <hr className="border-mid-gray/20" />,
  img: ({ alt, src }) => {
    if (!src || !isSafeImageSrc(src)) return null;

    return (
      <img
        src={src}
        alt={alt ?? ""}
        loading="lazy"
        decoding="async"
        className="mx-auto block max-h-72 max-w-full object-contain"
      />
    );
  },
  table: ({ children }) => (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-left text-sm leading-relaxed text-text/80">
        {children}
      </table>
    </div>
  ),
  thead: ({ children }) => (
    <thead className="border-b border-mid-gray/30 text-text">{children}</thead>
  ),
  tbody: ({ children }) => (
    <tbody className="divide-y divide-mid-gray/20">{children}</tbody>
  ),
  tr: ({ children }) => <tr>{children}</tr>,
  th: ({ children }) => (
    <th className="px-2 py-1.5 font-semibold">{children}</th>
  ),
  td: ({ children }) => <td className="px-2 py-1.5 align-top">{children}</td>,
  blockquote: ({ children }) => (
    <blockquote className="border-s-2 border-logo-primary/50 ps-3 text-sm leading-relaxed text-text/70">
      {children}
    </blockquote>
  ),
  code: ({ children, className }) => {
    const isBlock = className?.startsWith("language-");

    if (isBlock) {
      return (
        <code className="block whitespace-pre font-mono text-xs">
          {children}
        </code>
      );
    }

    return (
      <code className="rounded bg-mid-gray/10 px-1 py-0.5 font-mono text-[0.85em]">
        {children}
      </code>
    );
  },
  pre: ({ children }) => (
    <pre className="overflow-x-auto rounded-md bg-mid-gray/10 p-3 text-xs leading-relaxed text-text/80">
      {children}
    </pre>
  ),
  a: ({ children, href }) => {
    if (!href || !isSafeUrl(href)) {
      return <>{children}</>;
    }

    return (
      <a
        href={href}
        rel="noreferrer"
        onClick={(event) => {
          event.preventDefault();
          void openSafeUrl(href);
        }}
        className="text-logo-primary underline decoration-logo-primary/40 underline-offset-2 hover:decoration-logo-primary"
      >
        {children}
      </a>
    );
  },
};

export const MarkdownContent: React.FC<MarkdownContentProps> = ({
  markdown,
}) => {
  return (
    <div className="space-y-3">
      <ReactMarkdown
        allowedElements={allowedElements}
        components={components}
        remarkPlugins={[remarkGfm]}
        skipHtml
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
};
