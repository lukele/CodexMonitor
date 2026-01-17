#!/usr/bin/env node
/**
 * Pi Adapter - Bridges Codex app-server protocol to pi-coding-agent RPC
 * 
 * This adapter allows CodexMonitor to use pi-coding-agent as a backend,
 * enabling support for Claude and other models via pi's unified API.
 */

import { spawn, ChildProcess } from 'child_process';
import { createInterface, Interface } from 'readline';
import { readFile } from 'fs/promises';
import { homedir } from 'os';
import { join } from 'path';
import { v4 as uuidv4 } from 'uuid';

// Types for Codex protocol
interface CodexRequest {
  jsonrpc: '2.0';
  id: number | string;
  method: string;
  params?: Record<string, unknown>;
}

interface CodexResponse {
  jsonrpc: '2.0';
  id: number | string;
  result?: unknown;
  error?: { code: number; message: string };
}

interface CodexNotification {
  jsonrpc: '2.0';
  method: string;
  params?: Record<string, unknown>;
}

// Types for Pi RPC protocol
interface PiCommand {
  id?: string;
  type: string;
  [key: string]: unknown;
}

interface PiResponse {
  type: 'response';
  id?: string;
  command: string;
  success: boolean;
  data?: unknown;
  error?: string;
}

interface PiEvent {
  type: string;
  [key: string]: unknown;
}

// Claude OAuth rate limits fetcher
interface ClaudeOAuthCredentials {
  access: string;
  refresh: string;
  expires: number;
}

interface OAuthUsageWindow {
  utilization?: number;
  resets_at?: string;
}

interface OAuthUsageResponse {
  five_hour?: OAuthUsageWindow;
  seven_day?: OAuthUsageWindow;
  seven_day_sonnet?: OAuthUsageWindow;
  seven_day_opus?: OAuthUsageWindow;
  extra_usage?: {
    is_enabled?: boolean;
    monthly_limit?: number;
    used_credits?: number;
    currency?: string;
  };
}

async function loadClaudeCredentials(): Promise<ClaudeOAuthCredentials | null> {
  // Try pi's auth.json first
  const piAuthPath = join(homedir(), '.pi', 'agent', 'auth.json');
  try {
    const data = await readFile(piAuthPath, 'utf-8');
    const auth = JSON.parse(data);
    if (auth.anthropic?.access) {
      return {
        access: auth.anthropic.access,
        refresh: auth.anthropic.refresh,
        expires: auth.anthropic.expires
      };
    }
  } catch {
    // Fall through to Claude credentials
  }
  
  // Try Claude's credentials.json
  const claudeCredPath = join(homedir(), '.claude', '.credentials.json');
  try {
    const data = await readFile(claudeCredPath, 'utf-8');
    const creds = JSON.parse(data);
    if (creds.claudeAiOauth?.accessToken) {
      return {
        access: creds.claudeAiOauth.accessToken,
        refresh: creds.claudeAiOauth.refreshToken,
        expires: creds.claudeAiOauth.expiresAt
      };
    }
  } catch {
    // No credentials found
  }
  
  return null;
}

async function fetchClaudeRateLimits(): Promise<Record<string, unknown>> {
  const creds = await loadClaudeCredentials();
  if (!creds) {
    throw new Error('No Claude OAuth credentials found');
  }
  
  // Check if token is expired
  if (creds.expires && Date.now() > creds.expires) {
    throw new Error('Claude OAuth token expired');
  }
  
  const response = await fetch('https://api.anthropic.com/api/oauth/usage', {
    method: 'GET',
    headers: {
      'Authorization': `Bearer ${creds.access}`,
      'Accept': 'application/json',
      'Content-Type': 'application/json',
      'anthropic-beta': 'oauth-2025-04-20',
      'User-Agent': 'CodexMonitor-PiAdapter'
    }
  });
  
  if (!response.ok) {
    throw new Error(`OAuth API error: ${response.status}`);
  }
  
  const usage: OAuthUsageResponse = await response.json();
  
  // Transform to CodexMonitor's expected format
  const primary = usage.five_hour ? {
    usedPercent: usage.five_hour.utilization ?? 0,
    resetsAt: usage.five_hour.resets_at,
    windowMinutes: 5 * 60
  } : null;
  
  const secondary = usage.seven_day ? {
    usedPercent: usage.seven_day.utilization ?? 0,
    resetsAt: usage.seven_day.resets_at,
    windowMinutes: 7 * 24 * 60
  } : null;
  
  const credits = usage.extra_usage?.is_enabled ? {
    hasCredits: true,
    unlimited: false,
    balance: usage.extra_usage.used_credits !== undefined && usage.extra_usage.monthly_limit !== undefined
      ? `${((usage.extra_usage.monthly_limit - usage.extra_usage.used_credits) / 100).toFixed(2)}`
      : null
  } : { hasCredits: false, unlimited: false, balance: null };
  
  return { primary, secondary, credits };
}

// State
let piProcess: ChildProcess | null = null;
let piReader: Interface | null = null;
let currentThreadId: string | null = null;
let currentTurnId: string | null = null;
let currentMessageId: string | null = null;
let isProcessing = false;
let pendingRequests = new Map<string, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
let currentModel = 'claude-sonnet-4-20250514';
let currentProvider = 'anthropic';
let cwd = process.cwd();
// Cache tool args for use in tool_execution_end
let toolArgsCache = new Map<string, { toolName: string; args: Record<string, unknown> }>();
// Accumulated diff for current turn (sent as turn/diff/updated)
let accumulatedDiff: string[] = [];
// Map model ID to provider for proper set_model calls
let modelProviderMap = new Map<string, string>();

// Output helpers
function sendCodexResponse(id: number | string, result?: unknown, error?: { code: number; message: string }) {
  const response: CodexResponse = { jsonrpc: '2.0', id };
  if (error) {
    response.error = error;
  } else {
    response.result = result;
  }
  console.log(JSON.stringify(response));
}

function sendCodexNotification(method: string, params?: Record<string, unknown>) {
  const notification: CodexNotification = { jsonrpc: '2.0', method };
  if (params) {
    notification.params = params;
  }
  console.log(JSON.stringify(notification));
}

// Set to true for verbose debugging
const DEBUG = false;

function log(...args: unknown[]) {
  console.error('[pi-adapter]', ...args);
}

function debug(...args: unknown[]) {
  if (DEBUG) console.error('[pi-adapter]', ...args);
}

// Pi process management
async function ensurePiProcess(): Promise<void> {
  if (piProcess && !piProcess.killed) {
    return;
  }

  log('Starting pi process...');
  
  // Find pi - prefer monorepo version, then installed version
  const piMonorepo = process.env.PI_MONOREPO || `${process.env.HOME}/pi-antigravity/pi-mono/packages/coding-agent`;
  const piCliJs = `${piMonorepo}/dist/cli.js`;
  
  let piBin: string;
  let piArgs: string[];
  
  // Check if monorepo version exists
  try {
    await import('fs').then(fs => fs.promises.access(piCliJs));
    piBin = 'node';
    piArgs = [piCliJs, '--mode', 'rpc', '--no-session'];
    log('Using pi from monorepo:', piCliJs);
  } catch {
    // Fall back to installed pi
    piBin = process.env.PI_BIN || 'pi';
    piArgs = ['--mode', 'rpc', '--no-session'];
    log('Using installed pi:', piBin);
  }
  
  piProcess = spawn(piBin, piArgs, {
    cwd,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: {
      ...process.env,
      // Pass through API keys - pi will use these over auth.json OAuth tokens
      ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY,
      OPENAI_API_KEY: process.env.OPENAI_API_KEY,
      OPENCODE_API_KEY: process.env.OPENCODE_API_KEY,
      MISTRAL_API_KEY: process.env.MISTRAL_API_KEY,
      GOOGLE_API_KEY: process.env.GOOGLE_API_KEY,
      // Also set HOME so pi can find its config
      HOME: process.env.HOME,
    }
  });
  
  log('Environment: ANTHROPIC_API_KEY=' + (process.env.ANTHROPIC_API_KEY ? 'set' : 'not set'));

  piProcess.on('error', (err) => {
    log('Pi process error:', err.message);
  });

  piProcess.on('exit', (code) => {
    log('Pi process exited with code:', code);
    piProcess = null;
    piReader = null;
  });

  // Handle pi stderr (logs)
  if (piProcess.stderr) {
    const stderrReader = createInterface({ input: piProcess.stderr });
    stderrReader.on('line', (line) => {
      log('[pi stderr]', line);
    });
  }

  // Handle pi stdout (events/responses)
  if (piProcess.stdout) {
    piReader = createInterface({ input: piProcess.stdout });
    piReader.on('line', (line) => {
      try {
        const event = JSON.parse(line) as PiResponse | PiEvent;
        handlePiMessage(event);
      } catch (e) {
        log('Failed to parse pi output:', line);
      }
    });
  }

  // Wait a bit for process to initialize
  await new Promise(resolve => setTimeout(resolve, 100));
  
  // Set the model
  await sendPiCommand({ type: 'set_model', provider: 'anthropic', modelId: currentModel });
  
  log('Pi process started');
}

function sendPiCommand(command: PiCommand): Promise<unknown> {
  return new Promise((resolve, reject) => {
    if (!piProcess || !piProcess.stdin) {
      reject(new Error('Pi process not running'));
      return;
    }

    const id = command.id || uuidv4();
    command.id = id;
    
    pendingRequests.set(id, { resolve, reject });
    
    const line = JSON.stringify(command) + '\n';
    piProcess.stdin.write(line);
  });
}

function handlePiMessage(msg: PiResponse | PiEvent) {
  // Handle responses to our commands
  if (msg.type === 'response') {
    const response = msg as PiResponse;
    if (response.id && pendingRequests.has(response.id)) {
      const { resolve, reject } = pendingRequests.get(response.id)!;
      pendingRequests.delete(response.id);
      
      if (response.success) {
        resolve(response.data);
      } else {
        reject(new Error(response.error || 'Unknown error'));
      }
    }
    return;
  }

  // Handle events - translate to Codex protocol
  const event = msg as PiEvent;
  translatePiEventToCodex(event);
}

function translatePiEventToCodex(event: PiEvent) {
  const threadId = currentThreadId || 'default';
  const turnId = currentTurnId || uuidv4();

  debug('Pi event:', event.type, JSON.stringify(event).substring(0, 200));

  switch (event.type) {
    case 'agent_start':
      isProcessing = true;
      accumulatedDiff = []; // Reset accumulated diff for new turn
      sendCodexNotification('turn/started', {
        turn: {
          id: turnId,
          threadId,
          status: 'inProgress'
        }
      });
      break;

    case 'agent_end':
      isProcessing = false;
      sendCodexNotification('turn/completed', {
        turn: {
          id: turnId,
          threadId,
          status: 'completed'
        }
      });
      currentTurnId = null;
      break;

    case 'message_start': {
      const message = event.message as Record<string, unknown>;
      if (message?.role === 'assistant') {
        currentMessageId = uuidv4();
        sendCodexNotification('item/started', {
          threadId,
          item: {
            id: currentMessageId,
            type: 'agentMessage',
            status: 'inProgress'
          }
        });
      }
      break;
    }

    case 'message_update': {
      const assistantEvent = event.assistantMessageEvent as Record<string, unknown>;
      if (!assistantEvent) break;

      const eventType = assistantEvent.type as string;
      
      if (eventType === 'text_delta') {
        const delta = assistantEvent.delta as string;
        
        sendCodexNotification('item/agentMessage/delta', {
          threadId,
          itemId: currentMessageId || 'msg-0',
          delta
        });
      } else if (eventType === 'thinking_start') {
        // Send item/started for reasoning block
        sendCodexNotification('item/started', {
          threadId,
          item: {
            id: 'thinking',
            type: 'reasoning',
            status: 'inProgress'
          }
        });
      } else if (eventType === 'thinking_delta') {
        const delta = assistantEvent.delta as string;
        sendCodexNotification('item/reasoning/textDelta', {
          threadId,
          itemId: 'thinking',
          delta
        });
      } else if (eventType === 'thinking_end') {
        const content = assistantEvent.content as string || '';
        // Send item/completed for reasoning block with full content
        sendCodexNotification('item/completed', {
          threadId,
          item: {
            id: 'thinking',
            type: 'reasoning',
            status: 'completed',
            content: content,
            summary: content
          }
        });
      } else if (eventType === 'toolcall_end') {
        const toolCall = assistantEvent.toolCall as Record<string, unknown>;
        if (toolCall) {
          sendCodexNotification('item/started', {
            threadId,
            item: {
              id: toolCall.id,
              type: 'commandExecution',
              name: toolCall.name,
              arguments: toolCall.arguments,
              status: 'inProgress'
            }
          });
        }
      }
      break;
    }

    case 'message_end': {
      const message = event.message as Record<string, unknown>;
      if (message?.role === 'assistant') {
        const content = message.content as Array<Record<string, unknown>>;
        
        // Extract text content
        const textBlocks = content?.filter(b => b.type === 'text') || [];
        const text = textBlocks.map(b => b.text).join('\n');
        
        // Use the same ID we started with
        const messageId = currentMessageId || uuidv4();
        currentMessageId = null;
        
        sendCodexNotification('item/completed', {
          threadId,
          item: {
            id: messageId,
            type: 'agentMessage',
            text,
            status: 'completed'
          }
        });

        // Send token usage if available
        const usage = message.usage as Record<string, unknown>;
        if (usage) {
          sendCodexNotification('thread/tokenUsage/updated', {
            threadId,
            tokenUsage: {
              inputTokens: usage.input,
              outputTokens: usage.output,
              cacheReadTokens: usage.cacheRead,
              cacheWriteTokens: usage.cacheWrite,
            }
          });
        }
      }
      break;
    }

    case 'tool_execution_start': {
      const toolName = event.toolName as string;
      const toolCallId = event.toolCallId as string;
      const args = event.args as Record<string, unknown>;
      
      debug('Tool execution start:', toolName, 'args:', JSON.stringify(args));
      
      // Store args for later use in tool_execution_end
      toolArgsCache.set(toolCallId, { toolName, args });
      
      // Pi tool names: bash, read, edit, write, ls, find, grep
      const isCommand = toolName === 'bash' || toolName === 'Bash';
      const isFileChange = ['edit', 'Edit', 'write', 'Write'].includes(toolName);
      
      // Get relevant args based on tool type
      const command = (args?.command || args?.cmd) as string || '';
      const filePath = (args?.path || args?.file_path || args?.file || args?.filename) as string || '';
      const pattern = (args?.pattern) as string || '';
      
      let title: string;
      let detail: string;
      let toolType: string;
      
      if (isCommand) {
        toolType = 'commandExecution';
        title = `Command: ${command}`;
        detail = command;
      } else if (isFileChange) {
        // Edit/Write tools are file changes
        toolType = 'fileChange';
        title = `${toolName}: ${filePath}`;
        detail = filePath;
      } else {
        // Read-like tools (read, ls, find, grep) - show as commands with nice formatting
        toolType = 'commandExecution';
        let displayCmd: string;
        if (toolName === 'read' || toolName === 'Read') {
          displayCmd = `read ${filePath}`;
        } else if (toolName === 'ls') {
          displayCmd = `ls ${filePath || '.'}`;
        } else if (toolName === 'find') {
          displayCmd = `find "${pattern}" in ${filePath || '.'}`;
        } else if (toolName === 'grep') {
          displayCmd = `grep /${pattern}/ in ${filePath || '.'}`;
        } else {
          displayCmd = filePath ? `${toolName} ${filePath}` : toolName;
        }
        title = `Command: ${displayCmd}`;
        detail = displayCmd;
      }
      
      sendCodexNotification('item/started', {
        threadId,
        item: {
          id: toolCallId,
          type: toolType,
          toolType: toolType,
          // Use field names that buildConversationItem expects
          command: !isFileChange ? (isCommand ? command : detail) : undefined,
          cwd: cwd,
          status: 'inProgress',
          changes: isFileChange ? [{ path: filePath, kind: toolName.toLowerCase() === 'write' ? 'create' : 'edit' }] : undefined
        }
      });
      break;
    }

    case 'tool_execution_update': {
      const toolCallId = event.toolCallId as string;
      const partialResult = event.partialResult as Record<string, unknown>;
      const content = partialResult?.content as Array<Record<string, unknown>>;
      const text = content?.find(c => c.type === 'text');
      
      if (text) {
        sendCodexNotification('item/commandExecution/outputDelta', {
          threadId,
          itemId: toolCallId,
          delta: text.text
        });
      }
      break;
    }

    case 'tool_execution_end': {
      const toolName = event.toolName as string;
      const toolCallId = event.toolCallId as string;
      const result = event.result as Record<string, unknown>;
      const isError = event.isError as boolean;
      
      // Get cached args (more reliable than event.args which may be undefined)
      const cached = toolArgsCache.get(toolCallId);
      const args = cached?.args || event.args as Record<string, unknown> || {};
      toolArgsCache.delete(toolCallId);
      
      const content = result?.content as Array<Record<string, unknown>>;
      const text = content?.find(c => c.type === 'text');
      const details = result?.details as Record<string, unknown>;
      const diff = details?.diff as string;
      
      const isCommand = toolName === 'bash' || toolName === 'Bash';
      const isFileChange = ['edit', 'Edit', 'write', 'Write'].includes(toolName);
      
      const command = (args?.command || args?.cmd) as string || '';
      const filePath = (args?.path || args?.file_path || args?.file || args?.filename) as string || '';
      const pattern = (args?.pattern) as string || '';
      
      let title: string;
      let detail: string;
      let toolType: string;
      
      if (isCommand) {
        toolType = 'commandExecution';
        title = `Command: ${command}`;
        detail = command;
      } else if (isFileChange) {
        toolType = 'fileChange';
        title = `${toolName}: ${filePath}`;
        detail = filePath;
      } else {
        toolType = 'commandExecution';
        let displayCmd: string;
        if (toolName === 'read' || toolName === 'Read') {
          displayCmd = `read ${filePath}`;
        } else if (toolName === 'ls') {
          displayCmd = `ls ${filePath || '.'}`;
        } else if (toolName === 'find') {
          displayCmd = `find "${pattern}" in ${filePath || '.'}`;
        } else if (toolName === 'grep') {
          displayCmd = `grep /${pattern}/ in ${filePath || '.'}`;
        } else {
          displayCmd = filePath ? `${toolName} ${filePath}` : toolName;
        }
        title = `Command: ${displayCmd}`;
        detail = displayCmd;
      }
      
      sendCodexNotification('item/completed', {
        threadId,
        item: {
          id: toolCallId,
          type: toolType,
          toolType: toolType,
          // Use field names that buildConversationItem expects
          command: !isFileChange ? (isCommand ? command : detail) : undefined,
          cwd: cwd,
          aggregatedOutput: text?.text || '',
          exitCode: isError ? 1 : 0,
          status: isError ? 'error' : 'completed',
          changes: isFileChange ? [{ 
            path: filePath, 
            kind: toolName.toLowerCase() === 'write' ? 'create' : 'edit',
            diff: diff 
          }] : undefined
        }
      });
      
      // Accumulate diff for file changes and send turn/diff/updated
      debug('tool_execution_end file change check:', { isFileChange, hasDiff: !!diff, toolName, filePath });
      if (isFileChange) {
        if (diff) {
          const header = `--- a/${filePath}\n+++ b/${filePath}\n`;
          accumulatedDiff.push(header + diff);
        } else {
          // For new files without diff, create a simple addition diff
          const fileContent = text?.text as string || '';
          if (fileContent) {
            const lines = fileContent.split('\n').map(l => '+' + l).join('\n');
            const header = `--- /dev/null\n+++ b/${filePath}\n@@ -0,0 +1,${fileContent.split('\n').length} @@\n`;
            accumulatedDiff.push(header + lines);
          }
        }
        if (accumulatedDiff.length > 0) {
          debug('Sending turn/diff/updated with', accumulatedDiff.length, 'diffs');
          sendCodexNotification('turn/diff/updated', {
            threadId,
            diff: accumulatedDiff.join('\n\n')
          });
        }
      }
      break;
    }

    case 'auto_retry_start': {
      const errorMessage = event.errorMessage as string || 'Transient error';
      sendCodexNotification('error', {
        threadId,
        turnId,
        error: { message: `Retrying: ${errorMessage}` },
        willRetry: true
      });
      break;
    }

    case 'auto_retry_end': {
      const success = event.success as boolean;
      const finalError = event.finalError as string;
      if (!success && finalError) {
        sendCodexNotification('error', {
          threadId,
          turnId,
          error: { message: finalError },
          willRetry: false
        });
      }
      break;
    }

    case 'hook_error': {
      const hookPath = event.hookPath as string || '';
      const errorMsg = event.error as string || 'Hook error';
      sendCodexNotification('error', {
        threadId,
        turnId,
        error: { message: `Hook error (${hookPath}): ${errorMsg}` },
        willRetry: false
      });
      break;
    }

    default:
      debug('Unhandled pi event:', event.type);
  }
}

// Codex protocol handlers
async function handleCodexRequest(request: CodexRequest): Promise<void> {
  const { id, method, params } = request;
  
  debug(`Received: ${JSON.stringify(request).substring(0, 200)}`);
  debug(`Processing method: ${method}`);
  
  try {
    await ensurePiProcess();
    
    switch (method) {
      case 'initialize':
        sendCodexResponse(id, {
          protocolVersion: '2.0',
          capabilities: {
            threads: true,
            turns: true,
            models: true,
          }
        });
        break;

      case 'thread/start': {
        currentThreadId = uuidv4();
        cwd = (params?.cwd as string) || process.cwd();
        
        // Start a new session in pi (don't wait - it may not respond immediately)
        sendPiCommand({ type: 'new_session' }).catch(err => {
          debug('new_session error (ignored):', err);
        });
        
        sendCodexResponse(id, {
          thread: {
            id: currentThreadId,
            name: 'New Thread',
            createdAt: new Date().toISOString()
          }
        });
        break;
      }

      case 'thread/resume': {
        const threadId = params?.threadId as string;
        currentThreadId = threadId || uuidv4();
        
        sendCodexResponse(id, {
          thread: {
            id: currentThreadId,
            items: [],
            status: 'ready'
          }
        });
        break;
      }

      case 'thread/list':
        sendCodexResponse(id, {
          threads: currentThreadId ? [{
            id: currentThreadId,
            name: 'Current Session',
            createdAt: new Date().toISOString()
          }] : []
        });
        break;

      case 'thread/archive':
        currentThreadId = null;
        sendCodexResponse(id, { success: true });
        break;

      case 'turn/start': {
        debug('=== TURN START ===');
        debug('Params:', JSON.stringify(params));
        const threadId = params?.threadId as string;
        const input = params?.input as Array<Record<string, unknown>>;
        const model = params?.model as string;
        debug(`Thread: ${threadId}, Model: ${model}`);
        
        currentThreadId = threadId || currentThreadId || uuidv4();
        currentTurnId = uuidv4();
        
        // Extract text from input
        const textInputs = input?.filter(i => i.type === 'text') || [];
        const message = textInputs.map(i => i.text).join('\n');
        
        if (!message) {
          sendCodexResponse(id, undefined, { code: -32000, message: 'No text input provided' });
          return;
        }

        // Update model if specified
        if (model && model !== currentModel) {
          currentModel = model;
          
          // Model ID format: "provider/modelId" (e.g., "anthropic/claude-sonnet-4-20250514")
          let provider: string;
          let modelId: string;
          
          if (model.includes('/')) {
            // Composite ID format: provider/modelId
            const parts = model.split('/');
            provider = parts[0];
            modelId = parts.slice(1).join('/'); // Handle model IDs that might contain /
            log(`Parsed composite model ID: provider=${provider}, modelId=${modelId}`);
          } else {
            // Legacy format: just modelId, need to look up or guess provider
            modelId = model;
            provider = modelProviderMap.get(model) || '';
            debug(`Model lookup: ${model} -> cached provider: ${provider}, map size: ${modelProviderMap.size}`);
            if (!provider) {
              // Fallback: guess provider from model name
              if (model.startsWith('claude')) provider = 'anthropic';
              else if (model.startsWith('gpt') || model.startsWith('o1') || model.startsWith('o3')) provider = 'openai';
              else if (model.startsWith('gemini')) provider = 'google';
              else if (model.startsWith('mistral') || model.startsWith('codestral') || model.startsWith('devstral')) provider = 'mistral';
              else if (model.startsWith('grok') || model.includes('pickle') || model.includes('glm') || model.includes('minimax')) provider = 'opencode';
              else provider = 'anthropic'; // default
              debug(`Fallback provider guess: ${provider}`);
            }
          }
          
          currentProvider = provider;
          log(`Setting model: provider=${provider}, modelId=${modelId}`);
          try {
            await sendPiCommand({ type: 'set_model', provider, modelId });
            debug('Model set successfully');
          } catch (err) {
            log('Failed to set model:', (err as Error).message);
            throw err;
          }
        }

        // Send response immediately
        sendCodexResponse(id, {
          turn: {
            id: currentTurnId,
            items: [],
            status: 'inProgress'
          }
        });

        // Send prompt to pi (async - events will stream back)
        sendPiCommand({ type: 'prompt', message }).catch(err => {
          log('Prompt error:', err.message);
        });
        break;
      }

      case 'turn/interrupt':
      case 'thread/interrupt':
        await sendPiCommand({ type: 'abort' });
        isProcessing = false;
        sendCodexResponse(id, { success: true });
        break;

      case 'model/list': {
        try {
          const result = await sendPiCommand({ type: 'get_available_models' }) as { models: Array<Record<string, unknown>> } | undefined;
          debug('Raw model result:', JSON.stringify(result)?.substring(0, 500));
          const models = result?.models || [];
          log('Got models from pi:', models.length, 'models');
          
          // Store model info for later use when setting model
          // Use composite key: provider/modelId
          for (const m of models) {
            const compositeId = `${m.provider}/${m.id}`;
            modelProviderMap.set(compositeId, m.provider as string);
          }
          
          sendCodexResponse(id, {
            data: models.map(m => {
              const compositeId = `${m.provider}/${m.id}`;
              return {
                id: compositeId,
                model: m.id,
                provider: m.provider,
                displayName: m.name || m.id,
                description: `${m.provider} model`,
                supportedReasoningEfforts: m.reasoning ? [
                  { reasoningEffort: 'low', description: 'Light reasoning' },
                  { reasoningEffort: 'medium', description: 'Balanced' },
                  { reasoningEffort: 'high', description: 'Deep reasoning' }
                ] : [{ reasoningEffort: 'default', description: 'Standard' }],
                defaultReasoningEffort: m.reasoning ? 'medium' : 'default',
                isDefault: compositeId === currentModel,
                backend: 'pi'
              };
            })
          });
        } catch (err) {
          // Fallback to hardcoded list
          sendCodexResponse(id, {
            data: [
              {
                id: 'claude-opus-4-20250514',
                model: 'claude-opus-4-20250514',
                displayName: 'Claude Opus 4',
                supportedReasoningEfforts: [{ reasoningEffort: 'default', description: 'Standard' }],
                defaultReasoningEffort: 'default',
                isDefault: false,
                backend: 'claude'
              },
              {
                id: 'claude-sonnet-4-20250514',
                model: 'claude-sonnet-4-20250514',
                displayName: 'Claude Sonnet 4',
                supportedReasoningEfforts: [{ reasoningEffort: 'default', description: 'Standard' }],
                defaultReasoningEffort: 'default',
                isDefault: true,
                backend: 'claude'
              },
              {
                id: 'claude-3-5-sonnet-20241022',
                model: 'claude-3-5-sonnet-20241022',
                displayName: 'Claude 3.5 Sonnet',
                supportedReasoningEfforts: [{ reasoningEffort: 'default', description: 'Standard' }],
                defaultReasoningEffort: 'default',
                isDefault: false,
                backend: 'claude'
              }
            ]
          });
        }
        break;
      }

      case 'skills/list':
        sendCodexResponse(id, { skills: [] });
        break;

      case 'account/rateLimits':
      case 'account/rateLimits/read':
        debug('Rate limits requested');
        try {
          const rateLimits = await fetchClaudeRateLimits();
          debug('Rate limits fetched:', JSON.stringify(rateLimits));
          sendCodexResponse(id, { rateLimits });
        } catch (err) {
          log('Failed to fetch rate limits:', (err as Error).message);
          // Return empty/placeholder on error
          sendCodexResponse(id, {
            primary: null,
            secondary: null,
            credits: null
          });
        }
        break;

      case 'codex/respondToRequest': {
        // Handle approval responses
        const decision = params?.decision as string;
        // Pi handles approvals automatically, but we could extend this
        sendCodexResponse(id, { success: true });
        break;
      }

      case 'auth/status': {
        // Return list of authenticated providers from pi's auth.json
        try {
          const authPath = join(homedir(), '.pi', 'agent', 'auth.json');
          const authData = await readFile(authPath, 'utf-8');
          const auth = JSON.parse(authData);
          
          const providers = Object.entries(auth).map(([name, cred]: [string, any]) => ({
            name,
            type: cred.type || 'unknown',
            authenticated: true,
            expired: cred.expires ? Date.now() > cred.expires : false,
            expiresAt: cred.expires ? new Date(cred.expires).toISOString() : null
          }));
          
          // Add known OAuth providers that aren't authenticated
          const knownProviders = ['anthropic', 'openai-codex', 'github-copilot', 'google-gemini-cli', 'google-antigravity'];
          for (const p of knownProviders) {
            if (!providers.find(pr => pr.name === p)) {
              providers.push({
                name: p,
                type: 'oauth',
                authenticated: false,
                expired: false,
                expiresAt: null
              });
            }
          }
          
          sendCodexResponse(id, { providers });
        } catch (err) {
          log('Failed to get auth status:', (err as Error).message);
          sendCodexResponse(id, { providers: [] });
        }
        break;
      }

      case 'auth/login': {
        // Trigger OAuth login - this requires running pi interactively
        // For now, we return instructions since pi RPC doesn't expose login
        const provider = params?.provider as string;
        if (!provider) {
          sendCodexResponse(id, undefined, { code: -32602, message: 'provider parameter required' });
          break;
        }
        
        // pi doesn't have RPC login, so provide instructions
        sendCodexResponse(id, {
          success: false,
          message: `To authenticate with ${provider}, run: pi --provider ${provider}`,
          instructions: [
            `1. Open Terminal`,
            `2. Run: pi --provider ${provider}`,
            `3. Follow the OAuth flow in your browser`,
            `4. Return to CodexMonitor and reconnect the workspace`
          ]
        });
        break;
      }

      default:
        log('Unknown method:', method);
        sendCodexResponse(id, undefined, { code: -32601, message: `Method not found: ${method}` });
    }
    debug(`Completed method: ${method}`);
  } catch (err) {
    const error = err as Error;
    log('Error handling request:', method, error.message);
    sendCodexResponse(id, undefined, { code: -32000, message: error.message });
  }
}

// Main entry point
async function main() {
  log('Pi Adapter starting...');

  // Send initial connected notification
  sendCodexNotification('codex/connected', {});

  // Read JSON-RPC requests from stdin
  const reader = createInterface({ input: process.stdin });
  
  reader.on('line', async (line) => {
    if (!line.trim()) return;
    
    debug('Received:', line.substring(0, 100));
    
    try {
      const request = JSON.parse(line) as CodexRequest;
      
      // Be lenient - accept requests without jsonrpc field or with any version
      if (!request.method) {
        debug('Invalid request - no method field');
        return;
      }
      
      // Normalize the request to have jsonrpc field
      if (!request.jsonrpc) {
        request.jsonrpc = '2.0';
      }
      
      debug('Processing method:', request.method);
      await handleCodexRequest(request);
      debug('Completed method:', request.method);
    } catch (err) {
      const error = err as Error;
      log('Error processing request:', error.message, error.stack);
    }
  });

  reader.on('close', () => {
    log('Stdin closed, shutting down...');
    if (piProcess) {
      piProcess.kill();
    }
    process.exit(0);
  });

  // Handle signals
  process.on('SIGINT', () => {
    log('SIGINT received, shutting down...');
    if (piProcess) {
      piProcess.kill();
    }
    process.exit(0);
  });

  process.on('SIGTERM', () => {
    log('SIGTERM received, shutting down...');
    if (piProcess) {
      piProcess.kill();
    }
    process.exit(0);
  });
}

main().catch(err => {
  console.error('Fatal error:', err);
  process.exit(1);
});
