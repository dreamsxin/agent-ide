import Editor, { type OnMount } from '@monaco-editor/react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type * as Monaco from 'monaco-editor'
import './App.css'

type RuntimeCapability = {
  id: string
  label: string
}

type RuntimeBootstrap = {
  app_name: string
  runtime: string
  capabilities: RuntimeCapability[]
}

type WorkspaceEntry = {
  path: string
  name: string
  kind: 'file' | 'directory'
}

type WorkspaceState = {
  root: string
  entries: WorkspaceEntry[]
}

type FileDocument = {
  path: string
  contents: string
}

type GitChange = {
  path: string
  status: string
}

type GitState = {
  branch: string
  dirty: boolean
  summary: string
  changes: GitChange[]
}

type CommandResult = {
  command: string
  success: boolean
  exit_code: number | null
  stdout: string
  stderr: string
}

type LogEntry = {
  id: number
  level: 'info' | 'success' | 'error'
  message: string
}

type RuntimeLogEvent = {
  level: LogEntry['level']
  message: string
}

type OpenDocument = {
  path: string
  contents: string
  dirty: boolean
}

type CursorState = {
  line: number
  column: number
}

type TreeNode = {
  path: string
  name: string
  kind: 'file' | 'directory'
  children: TreeNode[]
}

type ActivityView = 'explorer' | 'editor' | 'review' | 'logs'

type ActivityItem = {
  id: ActivityView
  label: string
}

const ACTIVITY_ITEMS: ActivityItem[] = [
  { id: 'explorer', label: 'Explorer' },
  { id: 'editor', label: 'Editor' },
  { id: 'review', label: 'Review' },
  { id: 'logs', label: 'Logs' },
]

function App() {
  const [bootstrap, setBootstrap] = useState<RuntimeBootstrap | null>(null)
  const [workspace, setWorkspace] = useState<WorkspaceState | null>(null)
  const [gitState, setGitState] = useState<GitState | null>(null)
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [openDocuments, setOpenDocuments] = useState<Record<string, OpenDocument>>({})
  const [expandedDirs, setExpandedDirs] = useState<Record<string, boolean>>({})
  const [activeView, setActiveView] = useState<ActivityView>('explorer')
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [runningCommand, setRunningCommand] = useState(false)
  const [commandInput, setCommandInput] = useState('cargo check')
  const [lastCommandResult, setLastCommandResult] = useState<CommandResult | null>(null)
  const [message, setMessage] = useState('Pick a folder to start the IDE loop.')
  const [error, setError] = useState<string | null>(null)
  const [cursor, setCursor] = useState<CursorState>({ line: 1, column: 1 })
  const logIdRef = useRef(1)
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null)
  const [logs, setLogs] = useState<LogEntry[]>([
    {
      id: 0,
      level: 'info',
      message: 'Application booted. Waiting for workspace selection.',
    },
  ])

  const appendLog = useCallback((level: LogEntry['level'], entryMessage: string) => {
    const nextId = logIdRef.current
    logIdRef.current += 1

    setLogs((current) => [
      {
        id: nextId,
        level,
        message: entryMessage,
      },
      ...current,
    ].slice(0, 40))
  }, [])

  useEffect(() => {
    invoke<RuntimeBootstrap>('bootstrap_runtime')
      .then((payload) => {
        setBootstrap(payload)
        appendLog('success', 'Rust runtime bootstrap loaded.')
      })
      .catch((err) => {
        const nextError = String(err)
        setError(nextError)
        appendLog('error', nextError)
      })
  }, [appendLog])

  useEffect(() => {
    let unsubscribe: (() => void) | undefined

    void listen<RuntimeLogEvent>('runtime-log', (event) => {
      appendLog(event.payload.level, event.payload.message)
    }).then((fn) => {
      unsubscribe = fn
    })

    return () => unsubscribe?.()
  }, [appendLog])

  const chooseWorkspace = useCallback(async () => {
    try {
      setError(null)
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Open workspace',
      })

      if (!selected || Array.isArray(selected)) {
        appendLog('info', 'Workspace selection was cancelled.')
        return
      }

      setLoading(true)
      setMessage('Loading workspace...')
      appendLog('info', `Opening workspace: ${selected}`)

      const nextWorkspace = await invoke<WorkspaceState>('open_workspace', {
        path: selected,
      })
      const nextGitState = await invoke<GitState>('git_state', {
        root: nextWorkspace.root,
      })

      setWorkspace(nextWorkspace)
      setGitState(nextGitState)
      setSelectedPath(null)
      setOpenDocuments({})
      setExpandedDirs(buildExpandedMap(nextWorkspace.entries))
      setActiveView('explorer')
      setMessage(`Opened ${nextWorkspace.root}`)
      appendLog('success', `Workspace opened: ${nextWorkspace.root}`)
    } catch (err) {
      const nextError = String(err)
      setError(nextError)
      appendLog('error', nextError)
    } finally {
      setLoading(false)
    }
  }, [appendLog])

  const saveDocument = useCallback(async () => {
    if (!workspace || !selectedPath) return

    const activeDocument = openDocuments[selectedPath]
    if (!activeDocument) return

    setSaving(true)
    setError(null)
    appendLog('info', `Saving ${selectedPath}`)
    try {
      await invoke('save_file', {
        root: workspace.root,
        payload: {
          path: selectedPath,
          contents: activeDocument.contents,
        },
      })

      setOpenDocuments((current) => ({
        ...current,
        [selectedPath]: {
          ...current[selectedPath],
          dirty: false,
        },
      }))

      const nextGitState = await invoke<GitState>('git_state', {
        root: workspace.root,
      })
      setGitState(nextGitState)
      setMessage(`Saved ${selectedPath}`)
      appendLog('success', `Saved ${selectedPath}`)
    } catch (err) {
      const nextError = String(err)
      setError(nextError)
      appendLog('error', nextError)
    } finally {
      setSaving(false)
    }
  }, [appendLog, openDocuments, selectedPath, workspace])

  const runCommand = useCallback(async () => {
    if (!workspace || !commandInput.trim()) return

    setRunningCommand(true)
    setError(null)
    setActiveView('logs')
    try {
      const result = await invoke<CommandResult>('run_workspace_command', {
        root: workspace.root,
        payload: {
          command: commandInput.trim(),
        },
      })

      setLastCommandResult(result)
      setMessage(
        result.success
          ? `Command succeeded: ${result.command}`
          : `Command failed: ${result.command}`,
      )
    } catch (err) {
      const nextError = String(err)
      setError(nextError)
      appendLog('error', nextError)
    } finally {
      setRunningCommand(false)
    }
  }, [appendLog, commandInput, workspace])

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault()
        void saveDocument()
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [saveDocument])

  useEffect(() => {
    let unsubscribe: (() => void) | undefined

    void listen<string>('menu-action', (event) => {
      switch (event.payload) {
        case 'open-workspace':
          void chooseWorkspace()
          break
        case 'save-active':
          void saveDocument()
          break
        case 'focus-explorer':
          setActiveView('explorer')
          break
        case 'focus-editor':
          setActiveView('editor')
          break
        case 'focus-review':
          setActiveView('review')
          break
        case 'focus-logs':
          setActiveView('logs')
          break
        default:
          break
      }
    }).then((fn) => {
      unsubscribe = fn
    })

    return () => unsubscribe?.()
  }, [chooseWorkspace, saveDocument])

  function expandParents(path: string) {
    const parts = path.split('/')
    setExpandedDirs((current) => {
      const next = { ...current }
      for (let i = 1; i < parts.length; i += 1) {
        next[parts.slice(0, i).join('/')] = true
      }
      return next
    })
  }

  async function openDocument(path: string) {
    if (!workspace) return

    expandParents(path)

    if (openDocuments[path]) {
      setSelectedPath(path)
      setActiveView('editor')
      setMessage(`Editing ${path}`)
      return
    }

    setLoading(true)
    setError(null)
    appendLog('info', `Opening file: ${path}`)
    try {
      const document = await invoke<FileDocument>('read_file', {
        root: workspace.root,
        path,
      })

      setOpenDocuments((current) => ({
        ...current,
        [document.path]: {
          path: document.path,
          contents: document.contents,
          dirty: false,
        },
      }))
      setSelectedPath(document.path)
      setActiveView('editor')
      setMessage(`Editing ${document.path}`)
      appendLog('success', `Loaded ${document.path}`)
    } catch (err) {
      const nextError = String(err)
      setError(nextError)
      appendLog('error', nextError)
    } finally {
      setLoading(false)
    }
  }

  function updateDocumentContents(path: string, nextContents: string) {
    setOpenDocuments((current) => {
      const active = current[path]
      if (!active) return current

      return {
        ...current,
        [path]: {
          ...active,
          contents: nextContents,
          dirty: true,
        },
      }
    })
  }

  function closeDocument(path: string) {
    const isDirty = openDocuments[path]?.dirty
    if (isDirty) {
      const shouldClose = window.confirm(`Close ${labelForPath(path)} without saving?`)
      if (!shouldClose) return
    }

    const remaining = Object.keys(openDocuments).filter((item) => item !== path)
    setOpenDocuments((current) => {
      const next = { ...current }
      delete next[path]
      return next
    })

    if (selectedPath === path) {
      setSelectedPath(remaining[0] ?? null)
    }
  }

  function toggleDirectory(path: string) {
    setExpandedDirs((current) => ({
      ...current,
      [path]: !current[path],
    }))
  }

  const handleEditorMount: OnMount = (editor) => {
    editorRef.current = editor

    const position = editor.getPosition()
    if (position) {
      setCursor({
        line: position.lineNumber,
        column: position.column,
      })
    }

    editor.onDidChangeCursorPosition((event) => {
      setCursor({
        line: event.position.lineNumber,
        column: event.position.column,
      })
    })
  }

  const activeDocument = selectedPath ? openDocuments[selectedPath] : null
  const openTabs = useMemo(() => Object.values(openDocuments), [openDocuments])
  const workspaceTree = useMemo(() => buildTree(workspace?.entries ?? []), [workspace])
  const fileCount = useMemo(
    () => workspace?.entries.filter((entry) => entry.kind === 'file').length ?? 0,
    [workspace],
  )
  const selectedActivity = ACTIVITY_ITEMS.find((item) => item.id === activeView)
  const workspaceName = workspace ? workspace.root.split(/[\\/]/).pop() ?? workspace.root : 'No workspace'
  const breadcrumbs = useMemo(
    () => buildBreadcrumbs(workspaceName, activeDocument?.path ?? null),
    [activeDocument?.path, workspaceName],
  )
  const activeLanguage = activeDocument ? languageForPath(activeDocument.path) : 'plaintext'
  const activeEol = activeDocument?.contents.includes('\r\n') ? 'CRLF' : 'LF'

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">Desktop-first Agent IDE</p>
          <h1>Workbench with native menus and clearer layout</h1>
        </div>
        <div className="toolbar">
          <div className="command-bar">
            <input
              className="command-input"
              value={commandInput}
              onChange={(event) => setCommandInput(event.target.value)}
              placeholder="Run a workspace command"
              disabled={!workspace || runningCommand}
            />
            <button
              className="action-button secondary"
              onClick={() => void runCommand()}
              disabled={!workspace || !commandInput.trim() || runningCommand}
            >
              {runningCommand ? 'Running...' : 'Run'}
            </button>
          </div>
          <button className="action-button" onClick={() => void chooseWorkspace()} disabled={loading}>
            {loading ? 'Opening...' : 'Open Folder'}
          </button>
          <button
            className="action-button secondary"
            onClick={() => void saveDocument()}
            disabled={!selectedPath || saving}
            title="Ctrl+S"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </header>

      <section className="layout">
        <aside className="panel rail">
          <div className="rail-group">
            {ACTIVITY_ITEMS.map((item) => (
              <button
                key={item.id}
                className={`rail-item ${activeView === item.id ? 'active' : ''}`}
                title={item.label}
                aria-label={item.label}
                onClick={() => setActiveView(item.id)}
              >
                <span className="rail-symbol" aria-hidden="true">
                  <ActivityIcon view={item.id} />
                </span>
                <span className="rail-indicator" />
              </button>
            ))}
          </div>
        </aside>

        <aside className="panel sidebar">
          <div className="sidebar-header">
            <div>
              <p className="panel-label">{selectedActivity?.label ?? 'Explorer'}</p>
              <h2>{workspaceName}</h2>
            </div>
            <div className="workspace-meta">
              <span>{fileCount} files</span>
              <span>{gitState?.branch ?? 'no-git'}</span>
            </div>
          </div>

          {activeView === 'explorer' ? (
            <div className="explorer-toolbar">
              <span className="explorer-pill">{workspace ? 'Workspace loaded' : 'Waiting for folder'}</span>
              <span className="explorer-pill">{workspaceTree.length} root items</span>
            </div>
          ) : null}

          {activeView === 'explorer' ? (
            workspaceTree.length > 0 ? (
              <div className="tree-list">
                {workspaceTree.map((node) => (
                  <TreeItem
                    key={node.path}
                    node={node}
                    depth={0}
                    expandedDirs={expandedDirs}
                    selectedPath={selectedPath}
                    onToggleDir={toggleDirectory}
                    onOpenFile={openDocument}
                  />
                ))}
              </div>
            ) : (
              <p className="empty-copy">Use File {'>'} Open Folder or the toolbar button.</p>
            )
          ) : null}

          {activeView === 'editor' ? (
            <div className="sidebar-section">
              <p className="section-title">Open files</p>
              {openTabs.length > 0 ? (
                openTabs.map((tab) => (
                  <button
                    key={tab.path}
                    className={`sidebar-file ${tab.path === selectedPath ? 'active' : ''}`}
                    onClick={() => setSelectedPath(tab.path)}
                  >
                    <span>{tab.dirty ? '* ' : ''}{labelForPath(tab.path)}</span>
                    <code>{tab.path}</code>
                  </button>
                ))
              ) : (
                <p className="empty-copy">No open files yet.</p>
              )}
            </div>
          ) : null}

          {activeView === 'review' ? (
            <div className="sidebar-section">
              <p className="section-title">Changed files</p>
              {gitState?.changes.length ? (
                gitState.changes.map((change) => (
                  <div key={`${change.status}-${change.path}`} className="sidebar-file change">
                    <span>{change.path}</span>
                    <code>{change.status}</code>
                  </div>
                ))
              ) : (
                <p className="empty-copy">No Git changes to review.</p>
              )}
            </div>
          ) : null}

          {activeView === 'logs' ? (
            <div className="sidebar-section">
              {lastCommandResult ? (
                <div className={`command-summary ${lastCommandResult.success ? 'success' : 'error'}`}>
                  <strong>{lastCommandResult.success ? 'Last command succeeded' : 'Last command failed'}</strong>
                  <code>{lastCommandResult.command}</code>
                </div>
              ) : null}
              <p className="section-title">Recent runtime events</p>
              <div className="mini-log-list">
                {logs.slice(0, 10).map((entry) => (
                  <div key={entry.id} className={`mini-log ${entry.level}`}>
                    <strong>{entry.level}</strong>
                    <span>{entry.message}</span>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </aside>

        <section className="panel editor">
          <div className="editor-header">
            <div>
              <p className="panel-label">Workbench</p>
              <h2>{activeDocument?.path ?? 'Select a file'}</h2>
            </div>
            <div className="workspace-meta">
              <span>{bootstrap?.runtime ?? 'runtime'}</span>
              <span>{gitState?.summary ?? message}</span>
              <span>UTF-8 text rendering</span>
            </div>
          </div>

          <div className="breadcrumbs" aria-label="Breadcrumb">
            {breadcrumbs.map((crumb, index) => (
              <div key={`${crumb}-${index}`} className="breadcrumb-item">
                <span>{crumb}</span>
                {index < breadcrumbs.length - 1 ? <span className="breadcrumb-separator">/</span> : null}
              </div>
            ))}
          </div>

          <div className="tab-strip">
            {openTabs.length > 0 ? (
              openTabs.map((tab) => (
                <button
                  key={tab.path}
                  className={`tab-item ${tab.path === selectedPath ? 'active' : ''}`}
                  onClick={() => setSelectedPath(tab.path)}
                >
                  <span className="tab-title">
                    {tab.dirty ? '* ' : ''}
                    {labelForPath(tab.path)}
                  </span>
                  <span
                    className="tab-close"
                    onClick={(event) => {
                      event.stopPropagation()
                      closeDocument(tab.path)
                    }}
                  >
                    x
                  </span>
                </button>
              ))
            ) : (
              <div className="tab-empty">No files open</div>
            )}
          </div>

          {activeDocument ? (
            <Editor
              height="100%"
              path={activeDocument.path}
              language={languageForPath(activeDocument.path)}
              theme="vs-dark"
              value={activeDocument.contents}
              onMount={handleEditorMount}
              onChange={(value: string | undefined) =>
                updateDocumentContents(activeDocument.path, value ?? '')
              }
              options={{
                minimap: { enabled: false },
                fontSize: 14,
                padding: { top: 16 },
                lineHeight: 21,
                automaticLayout: true,
                smoothScrolling: true,
                cursorBlinking: 'smooth',
                glyphMargin: false,
                folding: true,
              }}
            />
          ) : (
            <div className="placeholder">
              <p>{message}</p>
              {error ? <p className="error-text">{error}</p> : null}
            </div>
          )}
        </section>

        <aside className="panel inspector">
          <p className="panel-label">Context</p>
          <h2>{bootstrap?.app_name ?? 'Agent IDE'}</h2>
          <ul className="capabilities">
            {(bootstrap?.capabilities ?? []).map((capability) => (
              <li key={capability.id}>
                <code>{capability.id}</code>
                <span>{capability.label}</span>
              </li>
            ))}
          </ul>
          <div className={`git-card ${gitState?.dirty ? 'dirty' : ''}`}>
            <strong>{gitState?.branch ?? 'no-git'}</strong>
            <p>{gitState?.summary ?? 'Git state appears here after opening a workspace.'}</p>
          </div>
          <div className="changes-card">
            <strong>Changed files</strong>
            {gitState?.changes.length ? (
              <div className="changes-list">
                {gitState.changes.slice(0, 12).map((change) => (
                  <div key={`${change.status}-${change.path}`} className="change-item">
                    <code>{change.status}</code>
                    <span>{change.path}</span>
                  </div>
                ))}
              </div>
            ) : (
              <p>No file-level Git changes to display.</p>
            )}
          </div>
        </aside>
      </section>

      <section className="panel log-panel">
        <div className="log-header">
          <div>
            <p className="panel-label">Logs</p>
            <h2>Runtime activity</h2>
          </div>
          <span className="workspace-meta">
            <span>{logs.length} entries</span>
          </span>
        </div>
        {lastCommandResult ? (
          <div className={`command-result ${lastCommandResult.success ? 'success' : 'error'}`}>
            <div className="command-result-header">
              <strong>{lastCommandResult.command}</strong>
              <span>
                {lastCommandResult.success ? 'Success' : 'Failed'}
                {lastCommandResult.exit_code !== null ? ` · exit ${lastCommandResult.exit_code}` : ''}
              </span>
            </div>
            {lastCommandResult.stdout ? (
              <div className="command-block">
                <p>stdout</p>
                <pre>{lastCommandResult.stdout}</pre>
              </div>
            ) : null}
            {lastCommandResult.stderr ? (
              <div className="command-block">
                <p>stderr</p>
                <pre>{lastCommandResult.stderr}</pre>
              </div>
            ) : null}
          </div>
        ) : null}
        <div className="log-list">
          {logs.map((entry) => (
            <div key={entry.id} className={`log-entry ${entry.level}`}>
              <strong>{entry.level.toUpperCase()}</strong>
              <p>{entry.message}</p>
            </div>
          ))}
        </div>
      </section>

      <footer className="statusbar">
        <span>{workspace ? workspace.root : 'No workspace selected'}</span>
        <span>{selectedPath ?? 'No file open'}</span>
        <span>{gitState?.branch ?? 'no-git'}</span>
        <span>{activeLanguage}</span>
        <span>{activeEol}</span>
        <span>UTF-8</span>
        <span>{selectedActivity?.label ?? 'Workbench'}</span>
        <span>
          Ln {cursor.line}, Col {cursor.column}
        </span>
        <span>{bootstrap?.runtime ?? 'runtime'}</span>
      </footer>
    </main>
  )
}

type TreeItemProps = {
  node: TreeNode
  depth: number
  expandedDirs: Record<string, boolean>
  selectedPath: string | null
  onToggleDir: (path: string) => void
  onOpenFile: (path: string) => void
}

function TreeItem({
  node,
  depth,
  expandedDirs,
  selectedPath,
  onToggleDir,
  onOpenFile,
}: TreeItemProps) {
  const isDirectory = node.kind === 'directory'
  const isExpanded = expandedDirs[node.path] ?? false

  return (
    <div className="tree-node">
      <button
        className={`tree-item ${selectedPath === node.path ? 'active' : ''}`}
        style={{ paddingLeft: `${12 + depth * 14}px` }}
        onClick={() => (isDirectory ? onToggleDir(node.path) : onOpenFile(node.path))}
      >
        <span className="tree-prefix" aria-hidden="true">
          {isDirectory ? (
            <svg viewBox="0 0 16 16" className={`chevron ${isExpanded ? 'expanded' : ''}`}>
              <path d="M5 3.5L10 8L5 12.5" />
            </svg>
          ) : (
            <span className="tree-prefix-spacer" />
          )}
        </span>
        <span className={`tree-icon ${isDirectory ? 'directory' : 'file'}`} aria-hidden="true">
          {isDirectory ? <FolderIcon open={isExpanded} /> : <FileIcon path={node.path} />}
        </span>
        <span className="tree-name">{node.name}</span>
      </button>
      {isDirectory && isExpanded ? (
        <div className="tree-children">
          {node.children.map((child) => (
            <TreeItem
              key={child.path}
              node={child}
              depth={depth + 1}
              expandedDirs={expandedDirs}
              selectedPath={selectedPath}
              onToggleDir={onToggleDir}
              onOpenFile={onOpenFile}
            />
          ))}
        </div>
      ) : null}
    </div>
  )
}

function ActivityIcon({ view }: { view: ActivityView }) {
  switch (view) {
    case 'explorer':
      return (
        <svg viewBox="0 0 24 24">
          <path d="M4 6.5h6l1.5 2H20v9.5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
          <path d="M4 8.5h16" />
        </svg>
      )
    case 'editor':
      return (
        <svg viewBox="0 0 24 24">
          <path d="M6 5h12a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z" />
          <path d="M8 9h8M8 12h8M8 15h5" />
        </svg>
      )
    case 'review':
      return (
        <svg viewBox="0 0 24 24">
          <path d="M7 5h10a2 2 0 0 1 2 2v10l-3-2-3 2-3-2-3 2V7a2 2 0 0 1 2-2z" />
          <path d="M9 10h6M9 13h4" />
        </svg>
      )
    case 'logs':
      return (
        <svg viewBox="0 0 24 24">
          <path d="M6 6h12a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z" />
          <path d="M8 10h8M8 13h5M8 16h7" />
        </svg>
      )
  }
}

function FolderIcon({ open }: { open: boolean }) {
  return open ? (
    <svg viewBox="0 0 24 24">
      <path d="M3.5 9.5h17l-1.7 7.5a2 2 0 0 1-2 1.5H7a2 2 0 0 1-2-1.5z" />
      <path d="M4 8V7a2 2 0 0 1 2-2h4l1.6 2H18a2 2 0 0 1 2 2" />
    </svg>
  ) : (
    <svg viewBox="0 0 24 24">
      <path d="M4 7a2 2 0 0 1 2-2h4l1.6 2H18a2 2 0 0 1 2 2v1H4z" />
      <path d="M4 10h16v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
    </svg>
  )
}

function FileIcon({ path }: { path: string }) {
  const variant = fileIconVariant(path)

  return (
    <svg viewBox="0 0 24 24" className={`file-icon file-icon-${variant}`}>
      <path d="M7 4.5h6l4 4V19a1.5 1.5 0 0 1-1.5 1.5h-8A1.5 1.5 0 0 1 6 19V6A1.5 1.5 0 0 1 7.5 4.5z" />
      <path d="M13 4.5V9h4" />
    </svg>
  )
}

function buildExpandedMap(entries: WorkspaceEntry[]): Record<string, boolean> {
  const result: Record<string, boolean> = {}
  for (const entry of entries) {
    if (entry.kind === 'directory') {
      result[entry.path] = entry.path.split('/').length <= 2
    }
  }
  return result
}

function buildTree(entries: WorkspaceEntry[]): TreeNode[] {
  const root: TreeNode[] = []
  const directoryMap = new Map<string, TreeNode>()

  for (const entry of entries) {
    const node: TreeNode = {
      path: entry.path,
      name: entry.name,
      kind: entry.kind,
      children: [],
    }

    if (entry.kind === 'directory') {
      directoryMap.set(entry.path, node)
    }

    const parentPath = parentOf(entry.path)
    if (!parentPath) {
      root.push(node)
      continue
    }

    const parent = directoryMap.get(parentPath)
    if (parent) {
      parent.children.push(node)
    } else {
      root.push(node)
    }
  }

  return sortTree(root)
}

function sortTree(nodes: TreeNode[]): TreeNode[] {
  return [...nodes]
    .sort((left, right) => {
      if (left.kind !== right.kind) return left.kind === 'directory' ? -1 : 1
      return left.name.localeCompare(right.name)
    })
    .map((node) => ({
      ...node,
      children: sortTree(node.children),
    }))
}

function parentOf(path: string): string | null {
  const index = path.lastIndexOf('/')
  return index === -1 ? null : path.slice(0, index)
}

function labelForPath(path: string): string {
  const parts = path.split('/')
  return parts[parts.length - 1] ?? path
}

function languageForPath(path: string): string {
  if (path.endsWith('.rs')) return 'rust'
  if (path.endsWith('.ts') || path.endsWith('.tsx')) return 'typescript'
  if (path.endsWith('.js') || path.endsWith('.jsx')) return 'javascript'
  if (path.endsWith('.json')) return 'json'
  if (path.endsWith('.md')) return 'markdown'
  if (path.endsWith('.css')) return 'css'
  if (path.endsWith('.html')) return 'html'
  if (path.endsWith('.toml')) return 'ini'
  if (path.endsWith('.yml') || path.endsWith('.yaml')) return 'yaml'
  return 'plaintext'
}

function fileIconVariant(path: string): string {
  if (path.endsWith('.rs')) return 'rust'
  if (path.endsWith('.ts') || path.endsWith('.tsx')) return 'ts'
  if (path.endsWith('.js') || path.endsWith('.jsx')) return 'js'
  if (path.endsWith('.json')) return 'json'
  if (path.endsWith('.md')) return 'md'
  if (path.endsWith('.css')) return 'css'
  if (path.endsWith('.html')) return 'html'
  return 'plain'
}

function buildBreadcrumbs(workspaceName: string, path: string | null): string[] {
  if (!path) return [workspaceName, 'workspace']
  return [workspaceName, ...path.split('/')]
}

export default App
