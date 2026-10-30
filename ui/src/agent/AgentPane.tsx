// The Agent pane: ask the desktop's agent (Claude Code, Codex) to build or change the
// report and model, and watch it work. Its changes land live and each request undoes in
// one step; its steps are listed as it goes, and Stop ends it. What it may see is set
// here too (Let the agent see data), and asked once per agent (AgentConsent).
import { Check, ChevronDown, ChevronRight, Database, MessageSquarePlus, Send, Sparkles, Square, SquareTerminal, Undo2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { AgentInfo } from "../bridge/generated/AgentInfo";
import type { Item } from "../bridge/generated/Item";
import type { Model } from "../bridge/generated/Model";
import type { Page } from "../bridge/generated/Page";
import type { AgentStep as Step } from "../bridge/generated/AgentStep";
import type { TurnRecord } from "../bridge/generated/TurnRecord";
import { formatNumber, useT } from "../i18n";
import { Button, IconButton } from "../kit/Button";
import { Menu } from "../kit/Menu";
import { Banner, EmptyState, Spinner } from "../kit/misc";
import { useAgent } from "../state/agent";
import { useApp, useProject } from "../state/app";
import { useReport, visualTitle } from "../state/report";
import { AgentConsent } from "./AgentConsent";
import "./agent.css";

const EXAMPLES = [
  "Build a page with sales by month as a line, sales by category as a donut and a region slicer.",
  "Add a measure for year-on-year growth in sales and show it as a card on this page.",
  "Tidy up this page: consistent titles, and arrange the visuals.",
];

export function AgentPane({ model, page }: { model: Model; page: Page }) {
  const { t } = useT();
  const status = useAgent((s) => s.status);
  const consent = useAgent((s) => s.consent);
  const request = useAgent((s) => s.request);
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void request({ type: "status" });
  }, [request]);
  const turns = status?.turns ?? [];
  const last = turns[turns.length - 1];
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [turns.length, last?.items.length, last?.status]);

  const agent = status?.agents.find((a) => a.name === status.agent) ?? null;
  return (
    <section className="pane agent-pane" aria-label={t("Agent")}>
      <div className="pane-head">
        <span>{t("Agent")}</span>
        <span className="agent-head-tools">
          <IconButton
            label={status?.data ? t("The agent can see data (rows and values); click to stop it") : t("Let the agent see data")}
            icon={<Database size={14} />}
            pressed={!!status?.data}
            onPress={() => void request({ type: "setData", on: !status?.data })}
          />
          <IconButton label={t("New chat")} icon={<MessageSquarePlus size={14} />} isDisabled={!turns.length || !!status?.running} onPress={() => void request({ type: "newChat" })} />
        </span>
      </div>
      {status && !status.agents.length ? (
        <div className="pane-body">
          <EmptyState icon={<Sparkles size={22} />} title={t("No agent is installed")}>
            {t("Install Claude Code or Codex (Omarchy menu → Install → AI), sign in to it once in a terminal, then come back.")}
          </EmptyState>
        </div>
      ) : (
        <>
          <AgentPicker agents={status?.agents ?? []} agent={agent} model={status?.model ?? ""} disabled={!!status?.running} />
          <div className="pane-body agent-transcript" aria-live="polite">
            {turns.length === 0 ? <Examples /> : turns.map((turn) => <TurnView key={turn.id} turn={turn} model={model} />)}
            <div ref={end} />
          </div>
          <Composer model={model} page={page} agent={agent} running={!!status?.running} />
        </>
      )}
      {consent && agent && (
        <AgentConsent
          agent={agent.label}
          data={!!status?.data}
          onCancel={() => useAgent.getState().set({ consent: null })}
          onAgree={async () => {
            useAgent.getState().set({ consent: null });
            if (await request({ type: "consent" })) await useAgent.getState().send(consent.text, consent.page, consent.visuals);
          }}
        />
      )}
    </section>
  );
}

function modelLabel(m: string, t: (s: string) => string) {
  return m ? m[0]!.toUpperCase() + m.slice(1) : t("Default model");
}

/** Which agent, and which of its models. */
function AgentPicker({ agents, agent, model, disabled }: { agents: AgentInfo[]; agent: AgentInfo | null; model: string; disabled: boolean }) {
  const { t } = useT();
  const request = useAgent((s) => s.request);
  const items = agents.flatMap((a) =>
    a.headless
      ? a.models.map((m) => ({
          id: `${a.name}:${m}`,
          label: m ? `${a.label} · ${modelLabel(m, t)}` : a.label,
          icon: a.name === agent?.name && m === model ? <Check size={14} /> : undefined,
          disabled,
          run: () => void request({ type: "setAgent", agent: a.name, model: m }),
        }))
      : [
          {
            id: `${a.name}:`,
            label: t("%1 (in a terminal)", a.label),
            icon: a.name === agent?.name ? <Check size={14} /> : <SquareTerminal size={14} />,
            disabled,
            run: () => void request({ type: "setAgent", agent: a.name, model: "" }),
          },
        ],
  );
  return (
    <div className="agent-picker">
      <Menu
        label={t("Agent and model")}
        trigger={
          <>
            <Sparkles size={14} />
            <span>{agent ? (agent.headless && model ? `${agent.label} · ${modelLabel(model, t)}` : agent.label) : t("Choose an agent")}</span>
            <ChevronDown size={13} />
          </>
        }
        items={items}
      />
    </div>
  );
}

function Examples() {
  const { t } = useT();
  return (
    <div className="agent-examples">
      <p className="pane-hint">{t("Ask for a page, a measure or a change. The agent works in this window: you see each change as it lands, and Undo takes back a whole request.")}</p>
      {EXAMPLES.map((e) => (
        <button key={e} type="button" className="agent-example" onClick={() => useAgent.getState().set({ draft: t(e) })}>
          {t(e)}
        </button>
      ))}
    </div>
  );
}

function TurnView({ turn, model }: { turn: TurnRecord; model: Model }) {
  const { t } = useT();
  const project = useProject();
  const running = turn.status === "running";
  const changed = turn.items.some((i) => i.kind === "step" && i.step.ok);
  const latest = project?.undoGroup === turn.group;
  return (
    <article className="agent-turn" aria-label={t("Request")}>
      <div className="agent-request">
        <p>{turn.request}</p>
        {turn.context && <span className="agent-chip">{turn.context}</span>}
      </div>
      <ol className="agent-items">
        {turn.items.map((item, i) => (
          <ItemView key={i} item={item} running={running} model={model} />
        ))}
      </ol>
      <div className="agent-turn-foot">
        {running ? (
          <span className="agent-working">
            <Spinner size={13} />
            {t("Working…")}
          </span>
        ) : turn.status === "stopped" ? (
          <span>{t("Stopped")}</span>
        ) : turn.status === "failed" ? (
          <span className="agent-failed">{t("Didn't finish")}</span>
        ) : null}
        {turn.costUsd != null && <span className="agent-cost">{formatNumber(turn.costUsd, { style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: 3 })}</span>}
        <span className="status-fill" />
        {changed && !running && (
          <Button
            size="sm"
            variant="ghost"
            icon={<Undo2 size={13} />}
            isDisabled={!latest}
            aria-description={latest ? undefined : t("Things have changed since; undo step by step instead (Ctrl+Z).")}
            onPress={() => void useApp.getState().rpc({ type: "undoGroup", group: turn.group })}
          >
            {t("Undo this request")}
          </Button>
        )}
      </div>
      {changed && !running && !latest && <p className="agent-note">{t("Things have changed since; undo step by step instead (Ctrl+Z).")}</p>}
    </article>
  );
}

function ItemView({ item, running, model }: { item: Item; running: boolean; model: Model }) {
  const { t } = useT();
  const request = useAgent((s) => s.request);
  switch (item.kind) {
    case "text":
      return <li className="agent-text">{item.text}</li>;
    case "step":
      return <StepLine step={item.step} model={model} />;
    case "error":
      return (
        <li>
          <Banner tone="danger">{item.message}</Banner>
        </li>
      );
    case "confirm":
      return (
        <li className="agent-confirm">
          <p>{t("The agent asks: %1?", item.what)}</p>
          {item.answer == null ? (
            running && (
              <div className="agent-confirm-actions">
                <Button size="sm" variant="ghost" onPress={() => void request({ type: "answer", id: item.id, allow: false })}>
                  {t("Skip")}
                </Button>
                <Button size="sm" variant="danger" onPress={() => void request({ type: "answer", id: item.id, allow: true })}>
                  {t("Allow")}
                </Button>
              </div>
            )
          ) : (
            <span className="agent-answer">{item.answer ? t("Allowed") : t("Skipped")}</span>
          )}
        </li>
      );
  }
}

function StepLine({ step, model }: { step: Step; model: Model }) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const text = t(step.op, ...step.args.map((a) => t(a)));
  const exists = step.visuals.filter((id) => model.report.pages.some((p) => p.visuals.some((v) => v.id === id)));
  const go = () => {
    const pageId = step.page && model.report.pages.some((p) => p.id === step.page) ? step.page : null;
    if (!pageId) return;
    useReport.getState().set({ page: pageId, selected: exists, view: "report" });
  };
  return (
    <li className={`agent-step ${step.ok ? "" : "is-failed"}`}>
      <span className="agent-step-row">
        {step.ok ? <Check size={13} className="agent-step-icon" /> : <X size={13} className="agent-step-icon" />}
        <button type="button" className="agent-step-text" onClick={go} disabled={!step.page}>
          {text}
        </button>
        {step.detail && (
          <IconButton label={open ? t("Hide details") : t("Show details")} icon={open ? <ChevronDown size={13} /> : <ChevronRight size={13} />} onPress={() => setOpen(!open)} className="agent-step-more" />
        )}
      </span>
      {open && step.detail && <pre className="agent-step-detail">{step.detail}</pre>}
    </li>
  );
}

/** The request box, with what the user is looking at as context they can drop. */
function Composer({ model, page, agent, running }: { model: Model; page: Page; agent: AgentInfo | null; running: boolean }) {
  const { t } = useT();
  const draft = useAgent((s) => s.draft);
  const selected = useReport((s) => s.selected);
  const [context, setContext] = useState(true);
  const [busy, setBusy] = useState(false);
  const visuals = page.visuals.filter((v) => selected.includes(v.id));
  const chip = visuals.length === 1 ? `${page.name} · ${visualTitle(model, visuals[0]!, t)}` : visuals.length ? t("%1 · %2 visuals", page.name, visuals.length) : page.name;
  const terminal = agent && !agent.headless;

  const send = async () => {
    const text = draft.trim();
    if (busy || running || !agent) return;
    setBusy(true);
    const ok = terminal
      ? await useAgent.getState().request({ type: "openTerminal", text })
      : text && (await useAgent.getState().send(text, context ? page.id : null, context ? visuals.map((v) => v.id) : []));
    setBusy(false);
    if (ok) {
      useAgent.getState().set({ draft: "" });
      setContext(true);
    }
  };

  return (
    <div className="agent-composer">
      {context && !terminal && (
        <span className="agent-chip">
          {chip}
          <button type="button" aria-label={t("Leave out the page and selection")} onClick={() => setContext(false)}>
            <X size={11} />
          </button>
        </span>
      )}
      <textarea
        className="input agent-input"
        aria-label={t("Ask the agent")}
        placeholder={terminal ? t("What should it do? (optional)") : t("Ask the agent to build or change something…")}
        value={draft}
        rows={3}
        onChange={(e) => useAgent.getState().set({ draft: e.target.value })}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            void send();
          }
        }}
      />
      <div className="agent-composer-actions">
        {terminal && <span className="field-hint">{t("%1 opens in a terminal with charted's tools.", agent.label)}</span>}
        <span className="status-fill" />
        {running ? (
          <Button size="sm" variant="secondary" icon={<Square size={13} />} onPress={() => void useAgent.getState().request({ type: "stop" })}>
            {t("Stop")}
          </Button>
        ) : terminal ? (
          <Button size="sm" variant="primary" icon={<SquareTerminal size={13} />} isDisabled={busy} onPress={send}>
            {t("Open in terminal")}
          </Button>
        ) : (
          <Button size="sm" variant="primary" icon={<Send size={13} />} isDisabled={busy || !draft.trim() || !agent} onPress={send}>
            {t("Send")}
          </Button>
        )}
      </div>
    </div>
  );
}
