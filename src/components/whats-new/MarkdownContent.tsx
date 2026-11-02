import React from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";

interface MarkdownContentProps {
  markdown: string;
}

const allowedElements = [
  "a",
  "blockquote",
  "br",
  "code",
  "em",
  "h1",
  "h2",
  "h3",
  "hr",
  "img",
  "li",
  "ol",
  "p",
  "pre",
  "strong",
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
    <p className="text-sm leading-relaxed text-text">{children}</p>
  ),
  ul: ({ children }) => (
    <ul className="list-disc space-y-1 ps-5 text-sm leading-relaxed text-text">
      {children}
    </ul>
  ),
  li: ({ children }) => <li className="pl-1 marker:text-ink-3">{children}</li>,
  ol: ({ children }) => (
    <ol className="list-decimal space-y-1 ps-5 text-sm leading-relaxed text-text">
      {children}
    </ol>
  ),
  br: () => <br />,
  hr: () => <hr className="border-line" />,
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
  blockquote: ({ children }) => (
    <blockquote className="border-s-2 border-logo-primary/50 ps-3 text-sm leading-relaxed text-ink-2">
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
      <code className="rounded bg-wash px-1 py-0.5 font-mono text-[0.85em]">
        {children}
      </code>
    );
  },
  pre: ({ children }) => (
    <pre className="overflow-x-auto rounded-md bg-wash p-3 text-xs leading-relaxed text-text">
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
        className="text-logo-primary underline decoration-logo-primary underline-offset-2 hover:decoration-logo-primary"
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
        skipHtml
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
};
