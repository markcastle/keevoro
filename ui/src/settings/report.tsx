// Help, Report a problem or suggest an idea: a few words for
// Keevoro's maker, and the reports sent from this computer with how each
// is getting on.
//
// Only the person's words, Keevoro's version, the operating system and the
// screen they were on go, unless they switch on the end of the log or a way
// to reach them. "See exactly what will be sent" shows the report as
// Keevoro made it, and that same text is what is sent. If sending fails,
// their words stay, with why and Try again.
//
// When Keevoro cannot reach the reports service at all (a work computer's
// proxy, say), the window sends the same text itself, through the
// system's own network settings, and Keevoro keeps the answer; it asks
// after reports the same way.
//
// A reporting code, if the person has one, is behind a quiet link. It is
// kept in the keychain by Keevoro and never comes back to the window: the
// page knows only whether one is kept. What is typed is kept only while it
// is typed, never in `kept`, and goes to the keychain before sending.

import { useCallback, useEffect, useId, useState } from 'react'
import type { Backend, ReportKind, ReportRequest, ReportsView, SentReport } from '../backend'
import { Toggle } from '../toggle'
import { askThroughWindow, checkedCode, ReportNotSent, sendThroughWindow } from '../reportsWindow'
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
  const [prepared, setPrepared] = useState<
    { asked: string; body: string; sendTo: string } | { asked: string; why: string } | null
  >(null)
  useEffect(() => {
    if (!seeing) return
    let open = true
    // Made again a moment after the typing stops, not at every letter.
    const timer = window.setTimeout(
      () =>
        api
          .prepareReport(JSON.parse(asked) as ReportRequest)
          .then((p) => open && setPrepared({ asked, body: p.body, sendTo: p.sendTo }))
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

  // Whether a reporting code is kept; what is being typed, if the field is
  // open; and why a code typed cannot be one.
  const [hasCode, setHasCode] = useState(false)
  const [codeOpen, setCodeOpen] = useState(false)
  const [code, setCode] = useState('')
  const [codeWhy, setCodeWhy] = useState<string | null>(null)
  useEffect(() => {
    let open = true
    api
      .reportingCode()
      .then((c) => open && setHasCode(c.hasCode))
      .catch(() => {})
    return () => {
      open = false
    }
  }, [api])
  const codeTyped = !hasCode && codeOpen && code.trim() !== ''
  /** Keeps the code typed, if one is; false when it cannot be one, said under the field. */
  const saveCode = async (): Promise<boolean> => {
    if (!codeTyped) return true
    const checked = checkedCode(code)
    if ('why' in checked) {
      setCodeWhy(checked.why)
      return false
    }
    try {
      const kept = await api.reportingCode({ code: checked.code })
      setHasCode(kept.hasCode)
      setCode('')
      setCodeWhy(null)
      return true
    } catch (e) {
      setCodeWhy(sentence(e))
      return false
    }
  }
  const forgetCode = async () => {
    try {
      const kept = await api.reportingCode({ forget: true })
      setHasCode(kept.hasCode)
      setCodeOpen(false)
      setCodeWhy(null)
    } catch (e) {
      setCodeWhy(sentence(e))
    }
  }

  const [sending, setSending] = useState(false)
  // Why it was not sent, and, when the service could not be reached at
  // all either way, why not in plain words if that is known.
  const [failed, setFailed] = useState<{ said: string; why?: string } | null>(null)
  const [sent, setSent] = useState<SentReport | null>(null)
  const [reports, setReports] = useState<ReportsView | null>(null)

  const refresh = useCallback(() => {
    let open = true
    api
      .reports()
      .then((r) => open && setReports((was) => was ?? r))
      .catch(() => {})
    const couldNot = () => open && setReports((was) => (was ? { ...was, checked: false, said: 'Couldn’t check just now.' } : was))
    api
      .refreshReports()
      .then(async (r) => {
        if (!open) return
        setReports(r)
        if (r.checked) return
        // Keevoro could not ask: the window asks itself.
        const asking = await api.reportsAsking()
        if (asking.reports.length === 0) return
        const { heard, all } = await askThroughWindow(asking.sendTo, asking.reports)
        if (heard.length === 0 && !all) return
        const view = await api.reportsHeard(heard, all)
        if (open) setReports(view)
      })
      .catch(couldNot)
    return () => {
      open = false
    }
  }, [api])
  useEffect(() => refresh(), [refresh])

  const send = async () => {
    // A code typed that cannot be one is refused before anything is sent.
    if (!(await saveCode())) return
    setSending(true)
    setFailed(null)
    try {
      // What is shown is what goes: the report on screen, if it is for
      // these words; otherwise made now, and shown if it is open.
      let made = prepared && 'body' in prepared && prepared.asked === asked ? prepared : null
      if (made === null) {
        const fresh = await api.prepareReport(request)
        made = { asked, body: fresh.body, sendTo: fresh.sendTo }
        setPrepared(made)
      }
      const done = await sendEitherWay(made.body, made.sendTo)
      setSent(done)
      kept = { kind, message: '', withLog: false, withContact, contact }
      setMessage('')
      setWithLog(false)
      setSeeing(false)
      setPrepared(null)
      api.reports().then(setReports, () => {})
    } catch (e) {
      setFailed(e instanceof ReportNotSent && e.unreachable ? { said: e.message, why: e.why } : { said: sentence(e) })
    } finally {
      setSending(false)
    }
  }

  // Keevoro sends it; when it cannot reach the service at all, the window
  // sends the very same text, and Keevoro keeps the answer.
  const sendEitherWay = async (body: string, sendTo: string): Promise<SentReport> => {
    try {
      return await api.sendReport(body)
    } catch (e) {
      if (!(e instanceof ReportNotSent) || !e.unreachable) throw e
      // With a reporting code, Keevoro hands over the report with it in.
      // It goes straight to the service from here and is never put in
      // state, shown or logged; Keevoro is told only the text shown.
      const answer = await sendThroughWindow(sendTo, e.sendAs ?? body)
      if (answer === null) throw e
      if ('why' in answer) throw new Error(answer.why)
      return api.noteReport(body, answer.reference, answer.token, answer.recognised)
    }
  }

  const words = useId()
  const reach = useId()
  const codeField = useId()
  const codeHelp = useId()
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
          {sent.codeSaid && <p className="small report-code-said">{sent.codeSaid}</p>}
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
              <Toggle row on={withLog} label="Include the end of Keevoro’s log" onChange={setWithLog} />
              <p className="quiet small">
                It helps to see what Keevoro was doing. The log can name your folders and files. Your home folder and
                user name are taken out, and it never holds keys or passwords.
              </p>
            </div>
            <div>
              <Toggle row on={withContact} label="Include a way to reach me" onChange={setWithContact} />
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

          <div className="report-code">
            {hasCode ? (
              <p className="small">
                <b>Reporting code</b> <span>saved on this computer.</span>{' '}
                <button type="button" className="link" onClick={() => void forgetCode()}>
                  Forget it
                </button>
              </p>
            ) : codeOpen ? (
              <div className="report-code-field">
                <label htmlFor={codeField}>Reporting code</label>
                <input
                  id={codeField}
                  type="password"
                  value={code}
                  autoComplete="off"
                  autoCapitalize="off"
                  spellCheck={false}
                  aria-describedby={codeHelp}
                  aria-invalid={codeWhy !== null}
                  onChange={(e) => {
                    setCode(e.target.value)
                    setCodeWhy(null)
                  }}
                  onKeyDown={(e) => {
                    // Return saves the code, rather than sending the report.
                    if (e.key === 'Enter') {
                      e.preventDefault()
                      void saveCode()
                    }
                  }}
                />
                <button type="button" className="button small" disabled={code.trim() === ''} onClick={() => void saveCode()}>
                  Save
                </button>
              </div>
            ) : (
              <button type="button" className="link quiet-link" onClick={() => setCodeOpen(true)}>
                I have a reporting code
              </button>
            )}
            {(hasCode || codeOpen) && (
              <p className="quiet small" id={codeHelp}>
                Optional. If Keevoro’s maker gave you one, your reports are recognised as yours.
              </p>
            )}
            {codeWhy && (
              <p className="small report-code-why" role="alert">
                {codeWhy}
              </p>
            )}
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
              <>
                <pre className="log" tabIndex={0} aria-label="Exactly what will be sent">
                  {prepared && 'body' in prepared ? prepared.body : ''}
                </pre>
                {/* The code itself is never shown: only that it goes. */}
                {(hasCode || codeTyped) && <p className="small report-code-line">Your reporting code</p>}
              </>
            )}
          </details>

          {failed && (
            <div className="report-failed" role="alert">
              <p>Your report wasn’t sent. {failed.said}</p>
              {failed.why && <p className="quiet small report-why">{failed.why}</p>}
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
                  <b>{r.reference}</b> <span className="report-kind">{r.kind === 'problem' ? 'Problem' : 'Idea'}</span>{' '}
                  <span>{r.firstLine}</span>
                  {r.recognised && <span className="report-recognised">Recognised</span>}
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

