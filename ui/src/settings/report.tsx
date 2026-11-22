// Settings, Help, Report a problem or suggest an idea: a few words for
// Keevoro's maker, and the reports sent from this computer with how each
// is getting on.
//
// Only the person's words, Keevoro's version, the operating system and the
// screen they were on go, unless they switch on the end of the log or a way
// to reach them. "See exactly what will be sent" shows the report as
// Keevoro made it, and that same text is what is sent. If sending fails,
// their words stay, with why and Try again.

import { useCallback, useEffect, useId, useState } from 'react'
import type { Backend, ReportKind, ReportRequest, ReportsView, SentReport } from '../backend'
import { Toggle } from '../toggle'
import { Group, Intro } from './parts'

/** What was being written, kept while Keevoro is open so leaving the page loses nothing. */
let kept: { kind: ReportKind; message: string; withLog: boolean; withContact: boolean; contact: string } = {
  kind: 'problem',
  message: '',
  withLog: false,
  withContact: false,
  contact: '',
}

const MOST = 5000

const sentence = (error: unknown) => (error instanceof Error ? error.message : String(error))

export function ReportPage(props: {
  api: Backend
  /** The screen the person was on when they asked to report something. */
  from: string
}) {
  const { api } = props
  const [kind, setKind] = useState(kept.kind)
  const [message, setMessage] = useState(kept.message)
  const [withLog, setWithLog] = useState(kept.withLog)
  const [withContact, setWithContact] = useState(kept.withContact)
  const [contact, setContact] = useState(kept.contact)
  useEffect(() => {
    kept = { kind, message, withLog, withContact, contact }
  }, [kind, message, withLog, withContact, contact])

  const request: ReportRequest = {
    kind,
    message,
    withLog,
    contact: withContact ? contact : null,
    screen: props.from,
  }
  const asked = JSON.stringify(request)

  // The report as Keevoro made it, for the request it was made from.
  const [seeing, setSeeing] = useState(false)
  const [prepared, setPrepared] = useState<{ asked: string; body: string } | { asked: string; why: string } | null>(null)
  useEffect(() => {
    if (!seeing) return
    let open = true
    // Made again a moment after the typing stops, not at every letter.
    const timer = window.setTimeout(
      () =>
        api
          .prepareReport(JSON.parse(asked) as ReportRequest)
          .then((p) => open && setPrepared({ asked, body: p.body }))
          .catch((e) => open && setPrepared({ asked, why: sentence(e) })),
      prepared === null ? 0 : 250,
    )
    return () => {
      open = false
      window.clearTimeout(timer)
    }
    // Only a change to what is asked, or opening it, makes it again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, asked, seeing])

  const [sending, setSending] = useState(false)
  const [failed, setFailed] = useState<string | null>(null)
  const [sent, setSent] = useState<SentReport | null>(null)
  const [reports, setReports] = useState<ReportsView | null>(null)

  const refresh = useCallback(() => {
    let open = true
    api
      .reports()
      .then((r) => open && setReports((was) => was ?? r))
      .catch(() => {})
    api
      .refreshReports()
      .then((r) => open && setReports(r))
      .catch(() => open && setReports((was) => (was ? { ...was, checked: false, said: 'Couldn’t check just now.' } : was)))
    return () => {
      open = false
    }
  }, [api])
  useEffect(() => refresh(), [refresh])

  const send = async () => {
    setSending(true)
    setFailed(null)
    try {
      // What is shown is what goes: the report on screen, if it is for
      // these words; otherwise made now, and shown if it is open.
      let body = prepared && 'body' in prepared && prepared.asked === asked ? prepared.body : null
      if (body === null) {
        const made = await api.prepareReport(request)
        body = made.body
        setPrepared({ asked, body })
      }
      const done = await api.sendReport(body)
      setSent(done)
      kept = { kind, message: '', withLog: false, withContact, contact }
      setMessage('')
      setWithLog(false)
      setSeeing(false)
      setPrepared(null)
      api.reports().then(setReports, () => {})
    } catch (e) {
      setFailed(sentence(e))
    } finally {
      setSending(false)
    }
  }

  const words = useId()
  const reach = useId()
  const letters = [...message].length
  const choices: { value: ReportKind; words: string }[] = [
    { value: 'problem', words: 'Something went wrong' },
    { value: 'idea', words: 'I have an idea' },
  ]

  return (
    <>
      <Intro>
        Tell Keevoro’s maker about something that went wrong, or an idea. Your report goes to a private list that only
        Keevoro’s maker reads.
      </Intro>

      {sent ? (
        <div className="report-thanks" role="status">
          <p>
            <b>{sent.said}</b>
          </p>
          <p className="quiet small">How it is getting on is under Your reports, below.</p>
          <p className="choices">
            <button type="button" className="button small" onClick={() => setSent(null)}>
              Write another
            </button>
          </p>
        </div>
      ) : (
        <form
          className="report-form"
          onSubmit={(e) => {
            e.preventDefault()
            void send()
          }}
        >
          <div className="segmented" role="radiogroup" aria-label="What is it?">
            {choices.map((choice) => (
              <button
                key={choice.value}
                type="button"
                role="radio"
                aria-checked={kind === choice.value}
                onClick={() => setKind(choice.value)}
              >
                {choice.words}
              </button>
            ))}
          </div>

          <label className="report-words" htmlFor={words}>
            <span>{kind === 'problem' ? 'What happened?' : 'What is your idea?'}</span>
            <textarea
              id={words}
              value={message}
              rows={6}
              maxLength={MOST}
              placeholder={
                kind === 'problem'
                  ? 'Such as: I pressed Back up now and nothing happened.'
                  : 'Such as: It would help to see which photos are new since last time.'
              }
              onChange={(e) => setMessage(e.target.value)}
            />
            {letters > MOST - 500 && (
              <small className="quiet">
                {letters.toLocaleString('en-GB')} of {MOST.toLocaleString('en-GB')} letters
              </small>
            )}
          </label>

          <div className="kv-panel report-switches">
            <div>
              <Toggle on={withLog} label="Include the end of Keevoro’s log" onChange={setWithLog} />
              <p className="quiet small">
                It helps to see what Keevoro was doing. The log can name your folders and files. Your home folder and
                user name are taken out, and it never holds keys or passwords.
              </p>
            </div>
            <div>
              <Toggle on={withContact} label="Include a way to reach me" onChange={setWithContact} />
              {withContact && (
                <label className="report-reach" htmlFor={reach}>
                  <span className="visually-hidden">How to reach you</span>
                  <input
                    id={reach}
                    value={contact}
                    maxLength={200}
                    placeholder="Such as an email address"
                    onChange={(e) => setContact(e.target.value)}
                  />
                </label>
              )}
            </div>
          </div>

          <p className="quiet small">
            Always sent with it: your words, Keevoro’s version, which system this computer runs, and the screen you were
            on ({props.from}). Nothing else.
          </p>

          <details className="report-preview" open={seeing} onToggle={(e) => setSeeing(e.currentTarget.open)}>
            <summary>See exactly what will be sent</summary>
            {prepared && prepared.asked === asked && 'why' in prepared ? (
              <p className="quiet small">{prepared.why}</p>
            ) : (
              <pre className="log" tabIndex={0} aria-label="Exactly what will be sent">
                {prepared && 'body' in prepared ? prepared.body : ''}
              </pre>
            )}
          </details>

          {failed && (
            <div className="report-failed" role="alert">
              <p>Your report wasn’t sent. {failed}</p>
              <p className="quiet small">Your words are still here.</p>
            </div>
          )}

          <p className="choices">
            <button type="submit" className="primary" disabled={sending || message.trim() === ''}>
              {sending ? 'Sending…' : failed ? 'Try again' : 'Send'}
            </button>
          </p>
        </form>
      )}

      {reports && reports.reports.length > 0 && (
        <Group
          title="Your reports"
          className="your-reports"
          footnote={reports.said ? <span role="status">{reports.said}</span> : undefined}
        >
          <ul id="setting-yourReports" className="report-list" tabIndex={-1}>
            {reports.reports.map((r) => (
              <li key={r.reference}>
                <span className="report-line">
                  <b>{r.reference}</b> <span>{r.firstLine}</span>
                </span>
                <span className={`report-status ${r.status ?? 'new'}`}>{r.statusSaid}</span>
                {r.note && <span className="quiet small report-note">{r.note}</span>}
              </li>
            ))}
          </ul>
        </Group>
      )}
    </>
  )
}

