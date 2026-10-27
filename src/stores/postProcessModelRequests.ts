import type { AppSettings, PostProcessApiFormat } from "@/bindings";

type Connection = readonly [
  baseUrl: string,
  apiFormat: PostProcessApiFormat,
  apiKey: string,
  modelsEndpoint: string | null,
];

// Compare the inputs to a model-list request, not unrelated settings or object
// identities. Keep credentials in memory only; never serialize this snapshot.
export function modelListConnection(
  settings: AppSettings | null,
  providerId: string,
): Connection | undefined {
  const provider = settings?.post_process_providers?.find(
    (p) => p.id === providerId,
  );
  if (!provider) return undefined;
  return [
    provider.base_url,
    provider.api_format ?? "openai",
    settings?.post_process_api_keys?.[providerId] ?? "",
    provider.models_endpoint ?? null,
  ];
}

export function sameModelListConnection(
  previous: Connection | undefined,
  current: Connection | undefined,
): boolean {
  return (
    previous === current ||
    (!!previous &&
      !!current &&
      previous.every((value, index) => value === current[index]))
  );
}

// One owner per provider: an obsolete completion cannot publish a result or
// release a newer request's loading flag. Reads requested during edits wait for
// all saves, so blurring a connection field and clicking refresh still works.
export class PostProcessModelRequests {
  private requests = new Map<string, { token: symbol; started: boolean }>();
  private edits = new Map<
    string,
    {
      count: number;
      settled: Promise<void>;
      resolve: () => void;
    }
  >();

  start(providerId: string): symbol {
    const request = Symbol();
    this.requests.set(providerId, { token: request, started: false });
    return request;
  }

  invalidate(providerId: string): void {
    // A queued read has not captured a connection yet. Keep that user intent;
    // it will use the latest settings once all pending edits have settled.
    if (this.requests.get(providerId)?.started)
      this.requests.delete(providerId);
  }

  hasCurrent(providerId: string): boolean {
    return this.requests.has(providerId);
  }

  pendingEdit(providerId: string): Promise<void> | undefined {
    return this.edits.get(providerId)?.settled;
  }

  markStarted(providerId: string, request: symbol): void {
    const current = this.requests.get(providerId);
    if (current?.token === request) current.started = true;
  }

  isCurrent(providerId: string, request: symbol): boolean {
    return this.requests.get(providerId)?.token === request;
  }

  finish(providerId: string, request: symbol): boolean {
    if (!this.isCurrent(providerId, request)) return false;
    this.requests.delete(providerId);
    return true;
  }

  beginEdit(providerId: string): () => void {
    this.invalidate(providerId);
    let edit = this.edits.get(providerId);
    if (!edit) {
      let resolve!: () => void;
      const settled = new Promise<void>((r) => {
        resolve = r;
      });
      edit = { count: 0, settled, resolve };
      this.edits.set(providerId, edit);
    }
    edit.count++;
    let ended = false;
    return () => {
      if (ended) return;
      ended = true;
      if (--edit.count === 0) {
        this.edits.delete(providerId);
        edit.resolve();
      }
    };
  }
}
