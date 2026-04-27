import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import { Input, Textarea } from '@heroui/input';
import {
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from '@heroui/modal';
import { Select, SelectItem } from '@heroui/select';
import { useLocalStorage } from '@uidotdev/usehooks';
import { useRequest } from 'ahooks';
import clsx from 'clsx';
import { useEffect, useMemo, useRef, useState } from 'react';
import toast from 'react-hot-toast';
import {
  LuBot,
  LuFile,
  LuFileText,
  LuImage,
  LuPlus,
  LuRefreshCw,
  LuSend,
  LuSettings2,
  LuTrash2,
  LuUser,
  LuX,
} from 'react-icons/lu';

import key from '@/const/key';

import LlmManager, {
  type LlmAttachmentKind,
  type LlmChatAttachment,
  type LlmConversationMessage,
  type LlmProviderCatalogItem,
} from '@/controllers/llm_manager';
import { useTheme } from '@/hooks/use-theme';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  attachments?: LlmChatAttachment[];
  meta?: string;
}

interface LlmChatConversation {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  providerLabel?: string;
  model?: string;
  messages: ChatMessage[];
}

interface LlmChatConversationsState {
  activeId: string;
  conversations: LlmChatConversation[];
}

interface LlmChatPreferences {
  baseUrl: string;
  model: string;
  temperature: string;
  topP: string;
  topK: string;
  frequencyPenalty: string;
  presencePenalty: string;
  reasoningEffort: string;
}

const DEFAULT_PREFERENCES: LlmChatPreferences = {
  baseUrl: '',
  model: '',
  temperature: '',
  topP: '',
  topK: '',
  frequencyPenalty: '',
  presencePenalty: '',
  reasoningEffort: '',
};

const DEFAULT_SUPPORTS = {
  streaming: false,
  temperature: true,
  topP: true,
  topK: true,
  frequencyPenalty: true,
  presencePenalty: true,
  reasoningEffort: true,
  imageInput: true,
  textFileInput: true,
  binaryFileInput: true,
};

const TEXT_FILE_EXTENSIONS = new Set([
  'txt',
  'md',
  'markdown',
  'json',
  'yaml',
  'yml',
  'toml',
  'ini',
  'log',
  'csv',
  'tsv',
  'xml',
  'html',
  'css',
  'scss',
  'js',
  'jsx',
  'ts',
  'tsx',
  'mjs',
  'cjs',
  'py',
  'rs',
  'go',
  'java',
  'kt',
  'sql',
  'sh',
  'bat',
  'ps1',
]);

const FILE_PICKER_ACCEPT = [
  '.txt',
  '.md',
  '.markdown',
  '.json',
  '.yaml',
  '.yml',
  '.toml',
  '.ini',
  '.log',
  '.csv',
  '.tsv',
  '.xml',
  '.html',
  '.css',
  '.scss',
  '.js',
  '.jsx',
  '.ts',
  '.tsx',
  '.mjs',
  '.cjs',
  '.py',
  '.rs',
  '.go',
  '.java',
  '.kt',
  '.sql',
  '.sh',
  '.bat',
  '.ps1',
].join(',');

function nextMessageId () {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function nextConversationId () {
  return `conversation-${nextMessageId()}`;
}

function createEmptyConversation (
  providerLabel?: string,
  model?: string
): LlmChatConversation {
  const now = Date.now();

  return {
    id: nextConversationId(),
    title: '新对话',
    createdAt: now,
    updatedAt: now,
    providerLabel,
    model,
    messages: [],
  };
}

function createConversationState (
  providerLabel?: string,
  model?: string
): LlmChatConversationsState {
  const conversation = createEmptyConversation(providerLabel, model);

  return {
    activeId: conversation.id,
    conversations: [conversation],
  };
}

function normalizeConversationState (
  state: LlmChatConversationsState | null | undefined,
  providerLabel?: string,
  model?: string
): LlmChatConversationsState {
  if (!state?.conversations?.length) {
    return createConversationState(providerLabel, model);
  }

  if (state.conversations.some((conversation) => conversation.id === state.activeId)) {
    return state;
  }

  return {
    ...state,
    activeId: state.conversations[0].id,
  };
}

function formatConversationTime (updatedAt: number) {
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(updatedAt));
}

function titleFromMessage (message: ChatMessage) {
  const content = message.content.trim();

  if (content) {
    return content.length > 18 ? `${content.slice(0, 18)}...` : content;
  }

  if (message.attachments?.length) {
    return `附件对话 ${message.attachments.length}`;
  }

  return '新对话';
}

function parseOptionalNumber (raw: string, parser: (value: string) => number) {
  const value = raw.trim();
  if (!value) {
    return undefined;
  }

  const parsed = parser(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function isTextLikeFile (file: File) {
  if (file.type.startsWith('text/')) {
    return true;
  }

  if (
    file.type.includes('json')
    || file.type.includes('xml')
    || file.type.includes('javascript')
  ) {
    return true;
  }

  const extension = file.name.split('.').pop()?.toLowerCase();
  return extension ? TEXT_FILE_EXTENSIONS.has(extension) : false;
}

function readFileAsDataUrl (file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ''));
    reader.onerror = () => reject(new Error(`读取文件失败: ${file.name}`));
    reader.readAsDataURL(file);
  });
}

function readFileAsText (file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ''));
    reader.onerror = () => reject(new Error(`读取文本失败: ${file.name}`));
    reader.readAsText(file, 'utf-8');
  });
}

async function createAttachment (file: File): Promise<LlmChatAttachment> {
  if (file.type.startsWith('image/')) {
    return {
      kind: 'image',
      name: file.name,
      mediaType: file.type || 'image/*',
      size: file.size,
      dataUrl: await readFileAsDataUrl(file),
    };
  }

  if (isTextLikeFile(file)) {
    return {
      kind: 'text',
      name: file.name,
      mediaType: file.type || 'text/plain',
      size: file.size,
      text: await readFileAsText(file),
    };
  }

  return {
    kind: 'file',
    name: file.name,
    mediaType: file.type || 'application/octet-stream',
    size: file.size,
    dataUrl: await readFileAsDataUrl(file),
  };
}

function detectProviderId (baseUrl: string) {
  const normalized = baseUrl.trim().toLowerCase();
  if (normalized.includes('api.openai.com')) {
    return 'openai';
  }
  if (normalized.includes('openrouter.ai')) {
    return 'openrouter';
  }
  if (normalized.includes('moonshot.ai')) {
    return 'kimi';
  }
  if (normalized.includes('dashscope.aliyuncs.com')) {
    return 'qwen';
  }
  if (normalized.includes('generativelanguage.googleapis.com')) {
    return 'google-gemini';
  }
  if (normalized.includes('anthropic.com')) {
    return 'anthropic';
  }
  return 'openai-compatible';
}

function acceptedFileTypes (supports: typeof DEFAULT_SUPPORTS) {
  const accepted: string[] = [];
  if (supports.imageInput) {
    accepted.push('image/*');
  }
  if (supports.textFileInput) {
    accepted.push(FILE_PICKER_ACCEPT);
  }
  return accepted.join(',');
}

function modelOptionsForProvider (
  providerId: string,
  providerCatalog: LlmProviderCatalogItem[]
) {
  return providerCatalog.find((item) => item.id === providerId)?.sampleModels ?? [];
}

function compactBaseUrl (value: string) {
  return value
    .replace(/^https?:\/\//, '')
    .replace(/\/v1\/?$/, '')
    .replace(/\/$/, '');
}

function getAttachmentIcon (kind: LlmAttachmentKind) {
  if (kind === 'image') {
    return LuImage;
  }

  if (kind === 'text') {
    return LuFileText;
  }

  return LuFile;
}

function AttachmentCard ({
  attachment,
  onRemove,
}: {
  attachment: LlmChatAttachment;
  onRemove?: () => void;
}) {
  const Icon = getAttachmentIcon(attachment.kind);

  return (
    <div className='group relative overflow-hidden rounded-2xl border border-white/10 bg-white/70 p-2 dark:bg-white/5'>
      {attachment.kind === 'image' && attachment.dataUrl
        ? (
          <div className='flex items-center gap-3'>
            <img
              src={attachment.dataUrl}
              alt={attachment.name}
              className='h-12 w-12 rounded-xl object-cover'
            />
            <div className='min-w-0'>
              <div className='truncate text-sm font-medium text-default-700 dark:text-default-100'>
                {attachment.name}
              </div>
              <div className='text-xs text-default-500'>图片附件</div>
            </div>
          </div>
          )
        : (
          <div className='flex items-center gap-3'>
            <div className='flex h-12 w-12 items-center justify-center rounded-xl bg-default-100/80 text-default-500 dark:bg-white/10 dark:text-default-300'>
              <Icon className='text-lg' />
            </div>
            <div className='min-w-0'>
              <div className='truncate text-sm font-medium text-default-700 dark:text-default-100'>
                {attachment.name}
              </div>
              <div className='text-xs text-default-500'>
                {attachment.kind === 'text' ? '文本附件' : '文件附件'}
              </div>
            </div>
          </div>
          )}

      {onRemove && (
        <button
          type='button'
          className='absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-full bg-black/60 text-white opacity-0 transition group-hover:opacity-100'
          onClick={onRemove}
        >
          <LuX className='text-sm' />
        </button>
      )}
    </div>
  );
}

function MessageBubble ({
  assistantBubbleClass,
  hasBackground,
  message,
}: {
  assistantBubbleClass: string;
  hasBackground: boolean;
  message: ChatMessage;
}) {
  const isUser = message.role === 'user';
  const isAssistant = message.role === 'assistant';
  const hasText = message.content.trim().length > 0;

  return (
    <div className='mx-auto w-full max-w-4xl'>
      <div className={clsx('flex', isUser ? 'justify-end' : isAssistant ? 'justify-start' : 'justify-center')}>
        <div
          className={clsx(
            'max-w-[92%] rounded-[26px] px-4 py-3 shadow-sm md:max-w-[78%]',
            isUser && 'bg-primary text-primary-foreground',
            isAssistant && assistantBubbleClass,
            message.role === 'system' && 'border border-danger/20 bg-danger/10 text-danger'
          )}
        >
          <div className='mb-2 flex items-center gap-2 text-xs opacity-80'>
            {isUser
              ? <LuUser />
              : isAssistant
                ? <LuBot />
                : <LuSettings2 />}
            <span>{isUser ? '你' : isAssistant ? 'LiteyukiBot' : '系统'}</span>
            {message.meta && <span className='truncate'>{message.meta}</span>}
          </div>

          {hasText && (
            <div className='whitespace-pre-wrap break-words text-sm leading-7'>
              {message.content}
            </div>
          )}

          {!hasText && message.attachments?.length
            ? (
              <div className='text-sm opacity-80'>
                已发送 {message.attachments.length} 个附件
              </div>
              )
            : null}

          {message.attachments?.length
            ? (
              <div className='mt-3 grid gap-2 sm:grid-cols-2'>
                {message.attachments.map((attachment, index) => (
                  <AttachmentCard
                    key={`${attachment.name}-${index}`}
                    attachment={attachment}
                  />
                ))}
              </div>
              )
            : null}
        </div>
      </div>
    </div>
  );
}

export default function LlmChatPage () {
  const { isDark } = useTheme();
  const [storedConfig, setStoredConfig] = useLocalStorage<LlmChatPreferences>(
    key.llmChatConfig,
    DEFAULT_PREFERENCES
  );
  const initialConversationStateRef = useRef<LlmChatConversationsState | null>(null);

  if (!initialConversationStateRef.current) {
    initialConversationStateRef.current = createConversationState();
  }

  const [storedConversationState, setStoredConversationState] = useLocalStorage<LlmChatConversationsState>(
    key.llmChatConversations,
    initialConversationStateRef.current
  );
  const config = storedConfig ?? DEFAULT_PREFERENCES;
  const [draft, setDraft] = useState('');
  const [attachments, setAttachments] = useState<LlmChatAttachment[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [backgroundImage] = useLocalStorage<string>(key.backgroundImage, '');
  const hasBackground = !!backgroundImage;
  const fileInputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const {
    data: settings,
    loading: settingsLoading,
    error: settingsError,
    refreshAsync: refreshSettings,
  } = useRequest(LlmManager.getSettings);

  useEffect(() => {
    if (!settings) {
      return;
    }

    setStoredConfig((current) => {
      const base = current ?? DEFAULT_PREFERENCES;

      return {
        ...DEFAULT_PREFERENCES,
        ...base,
        baseUrl: settings.baseUrl,
        model: settings.model,
      };
    });
  }, [settings, setStoredConfig]);

  useEffect(() => {
    if (!settingsError) {
      return;
    }

    toast.error(`加载模型配置失败: ${(settingsError as Error).message}`);
  }, [settingsError]);

  const conversationState = useMemo(
    () => normalizeConversationState(storedConversationState),
    [storedConversationState]
  );
  const currentConversation = useMemo(
    () => conversationState.conversations.find(
      (conversation) => conversation.id === conversationState.activeId
    ) ?? conversationState.conversations[0],
    [conversationState]
  );
  const activeConversationId = currentConversation?.id ?? conversationState.activeId;
  const messages = currentConversation?.messages ?? [];

  useEffect(() => {
    listRef.current?.scrollTo({
      top: listRef.current.scrollHeight,
      behavior: 'smooth',
    });
  }, [messages]);

  const providerOptions = useMemo(
    () => settings?.providerOptions ?? [],
    [settings]
  );
  const providerCatalog = useMemo(
    () => settings?.providerCatalog ?? [],
    [settings]
  );
  const currentBaseUrl = config.baseUrl || settings?.baseUrl || '';
  const currentModel = config.model || settings?.model || '';
  const selectedProviderOption = useMemo(
    () => providerOptions.find((option) => option.baseUrl === currentBaseUrl),
    [currentBaseUrl, providerOptions]
  );
  const selectedProviderId = useMemo(
    () => detectProviderId(currentBaseUrl),
    [currentBaseUrl]
  );
  const modelOptions = useMemo(() => {
    if (selectedProviderOption?.modelOptions?.length) {
      return selectedProviderOption.modelOptions;
    }
    if (settings?.baseUrl === currentBaseUrl && settings.modelOptions?.length) {
      return settings.modelOptions;
    }
    return modelOptionsForProvider(selectedProviderId, providerCatalog);
  }, [currentBaseUrl, providerCatalog, selectedProviderId, selectedProviderOption, settings]);
  const reasoningOptions = useMemo(() => {
    return settings?.reasoningOptions ?? [];
  }, [settings]);
  const supports = useMemo(() => ({
    ...DEFAULT_SUPPORTS,
    ...(settings?.supports ?? {}),
  }), [settings]);
  const chatEnabled = settings?.enabled;
  const chatReady = chatEnabled === true;
  const statusTone = settingsLoading && !settings
    ? 'default'
    : chatReady
      ? 'success'
      : 'danger';
  const statusText = settingsLoading && !settings
    ? 'LLM Loading'
    : chatReady
      ? 'LLM Ready'
      : 'LLM Disabled';
  const currentProviderLabel = providerOptions.find(
    (option) => option.baseUrl === currentBaseUrl
  )?.label || settings?.provider || (currentBaseUrl ? compactBaseUrl(currentBaseUrl) : '未配置 Provider');
  const acceptedAttachments = acceptedFileTypes(supports);
  const canSend = chatReady && !submitting && (draft.trim().length > 0 || attachments.length > 0);
  const themePanelClass = hasBackground
    ? isDark
      ? 'border-white/20 bg-black/30 text-white'
      : 'border-black/10 bg-white/72 text-default-700'
    : 'border-white/40 bg-white/60 text-default-700 dark:border-white/10 dark:bg-black/30 dark:text-default-100';
  const themeSurfaceClass = hasBackground
    ? isDark
      ? 'border-white/12 bg-black/25'
      : 'border-black/8 bg-white/58'
    : 'border-white/40 bg-white/60 dark:border-white/10 dark:bg-black/30';
  const themeSubtleSurfaceClass = hasBackground
    ? isDark
      ? 'border-white/10 bg-white/5'
      : 'border-black/8 bg-white/45'
    : 'border-white/20 bg-white/30 dark:bg-white/5';
  const mutedTextClass = hasBackground
    ? isDark
      ? 'text-white/70'
      : 'text-default-500'
    : 'text-default-500 dark:text-default-400';
  const quietTextClass = hasBackground
    ? isDark
      ? 'text-white/60'
      : 'text-default-400'
    : 'text-default-400 dark:text-default-500';
  const faintTextClass = hasBackground
    ? isDark
      ? 'text-white/50'
      : 'text-default-400'
    : 'text-default-400 dark:text-default-500';
  const strongTextClass = hasBackground
    ? isDark
      ? 'text-white'
      : 'text-default-700'
    : 'text-default-700 dark:text-default-100';
  const assistantBubbleClass = hasBackground
    ? isDark
      ? 'border border-white/12 bg-black/25 text-white'
      : 'border border-black/10 bg-white/74 text-default-700'
    : 'border border-white/20 bg-white/85 text-default-700 dark:border-white/10 dark:bg-black/30 dark:text-default-100';
  const composerShellClass = hasBackground
    ? isDark
      ? 'border-white/15 bg-black/22'
      : 'border-black/10 bg-white/76'
    : 'border-default-200/60 bg-white/80 dark:border-white/10 dark:bg-black/25';
  const composerInputClass = hasBackground
    ? isDark
      ? 'text-white placeholder:text-white/45'
      : 'text-default-700 placeholder:text-default-400'
    : 'text-default-700 placeholder:text-default-400 dark:text-default-100 dark:placeholder:text-default-500';

  useEffect(() => {
    if (!supports.reasoningEffort) {
      if (config.reasoningEffort) {
        setConfigField('reasoningEffort', '');
      }
      return;
    }

    if (config.reasoningEffort && reasoningOptions.length > 0 && !reasoningOptions.includes(config.reasoningEffort)) {
      setConfigField('reasoningEffort', '');
    }
  }, [config.reasoningEffort, reasoningOptions, supports.reasoningEffort]);

  const setConfigField = <K extends keyof LlmChatPreferences> (
    field: K,
    value: LlmChatPreferences[K]
  ) => {
    setStoredConfig((current) => ({
      ...(current ?? DEFAULT_PREFERENCES),
      [field]: value,
    }));
  };

  const updateStoredConversationState = (
    updater: (state: LlmChatConversationsState) => LlmChatConversationsState
  ) => {
    setStoredConversationState((current) => updater(normalizeConversationState(current)));
  };

  const updateConversationById = (
    conversationId: string,
    updater: (conversation: LlmChatConversation) => LlmChatConversation
  ) => {
    updateStoredConversationState((state) => ({
      ...state,
      conversations: state.conversations.map((conversation) => (
        conversation.id === conversationId ? updater(conversation) : conversation
      )),
    }));
  };

  const replaceConversationMessages = (
    conversationId: string,
    nextMessages: ChatMessage[]
  ) => {
    const firstUserMessage = nextMessages.find((message) => message.role === 'user');

    updateConversationById(conversationId, (conversation) => ({
      ...conversation,
      title: conversation.title === '新对话' && firstUserMessage
        ? titleFromMessage(firstUserMessage)
        : conversation.title,
      updatedAt: Date.now(),
      providerLabel: currentProviderLabel,
      model: currentModel,
      messages: nextMessages,
    }));
  };

  const handleCreateConversation = () => {
    const conversation = createEmptyConversation(currentProviderLabel, currentModel);

    setStoredConversationState((current) => {
      const state = normalizeConversationState(current);

      return {
        activeId: conversation.id,
        conversations: [conversation, ...state.conversations],
      };
    });
    setDraft('');
    setAttachments([]);
    toast.success('已创建新对话');
  };

  const handleSwitchConversation = (conversationId: string) => {
    if (conversationId === activeConversationId) {
      return;
    }

    updateStoredConversationState((state) => ({
      ...state,
      activeId: conversationId,
    }));
    setDraft('');
    setAttachments([]);
  };

  const handleDeleteConversation = (conversationId: string) => {
    setStoredConversationState((current) => {
      const state = normalizeConversationState(current);
      const deleteIndex = state.conversations.findIndex(
        (conversation) => conversation.id === conversationId
      );
      const remaining = state.conversations.filter(
        (conversation) => conversation.id !== conversationId
      );

      if (remaining.length === 0) {
        const nextConversation = createEmptyConversation(currentProviderLabel, currentModel);

        return {
          activeId: nextConversation.id,
          conversations: [nextConversation],
        };
      }

      return {
        activeId: state.activeId === conversationId
          ? remaining[Math.min(Math.max(deleteIndex, 0), remaining.length - 1)].id
          : state.activeId,
        conversations: remaining,
      };
    });

    if (conversationId === activeConversationId) {
      setDraft('');
      setAttachments([]);
    }

    toast.success('对话已删除');
  };

  const handleResetConfig = () => {
    if (!settings) {
      return;
    }

    setStoredConfig({
      ...DEFAULT_PREFERENCES,
      baseUrl: settings.baseUrl,
      model: settings.model,
    });
    toast.success('模型设置已重置');
  };

  const appendFiles = async (fileList: File[]) => {
    if (!fileList.length) {
      return;
    }

    try {
      const unsupported: string[] = [];
      const accepted: File[] = [];

      for (const file of fileList) {
        if (file.type.startsWith('image/')) {
          if (!supports.imageInput) {
            unsupported.push(`${file.name}: 当前 Provider 不支持图片输入`);
            continue;
          }
          accepted.push(file);
          continue;
        }

        if (isTextLikeFile(file)) {
          if (!supports.textFileInput) {
            unsupported.push(`${file.name}: 当前 Provider 不支持文本附件`);
            continue;
          }
          accepted.push(file);
          continue;
        }

        unsupported.push(`${file.name}: 当前后端路由暂不支持普通二进制文件`);
      }

      if (unsupported.length > 0) {
        toast.error(unsupported[0]);
      }

      if (accepted.length === 0) {
        return;
      }

      const nextAttachments = await Promise.all(accepted.map(createAttachment));
      setAttachments((current) => [...current, ...nextAttachments]);
      toast.success(`已添加 ${nextAttachments.length} 个附件`);
    } catch (error) {
      toast.error((error as Error).message);
    }
  };

  const handlePaste = (event: React.ClipboardEvent<HTMLInputElement>) => {
    const files = Array.from(event.clipboardData.items)
      .map((item) => item.getAsFile())
      .filter((file): file is File => !!file);

    if (!files.length) {
      return;
    }

    event.preventDefault();
    void appendFiles(files);
  };

  const handleSend = async () => {
    const trimmedDraft = draft.trim();

    if (!trimmedDraft && attachments.length === 0) {
      toast.error('请输入消息或添加附件');
      return;
    }

    const outgoingAttachments = attachments;
    const userMessage: ChatMessage = {
      id: nextMessageId(),
      role: 'user',
      content: trimmedDraft,
      attachments: outgoingAttachments,
    };
    const targetConversationId = activeConversationId;
    const optimisticMessages = [...messages, userMessage];
    const history: LlmConversationMessage[] = optimisticMessages.map((message) => ({
      role: message.role,
      content: message.content,
      attachments: message.attachments,
    }));

    replaceConversationMessages(targetConversationId, optimisticMessages);
    setDraft('');
    setAttachments([]);
    setSubmitting(true);

    try {
      const response = await LlmManager.chat({
        message: trimmedDraft,
        messages: history,
        attachments: outgoingAttachments,
        baseUrl: currentBaseUrl || undefined,
        model: currentModel || undefined,
        temperature: supports.temperature ? parseOptionalNumber(config.temperature, Number) : undefined,
        topP: supports.topP ? parseOptionalNumber(config.topP, Number) : undefined,
        topK: supports.topK ? parseOptionalNumber(config.topK, (value) => parseInt(value, 10)) : undefined,
        frequencyPenalty: supports.frequencyPenalty ? parseOptionalNumber(config.frequencyPenalty, Number) : undefined,
        presencePenalty: supports.presencePenalty ? parseOptionalNumber(config.presencePenalty, Number) : undefined,
        reasoningEffort: supports.reasoningEffort ? (config.reasoningEffort || undefined) : undefined,
      });

      replaceConversationMessages(
        targetConversationId,
        [
          ...optimisticMessages,
        {
          id: nextMessageId(),
          role: 'assistant',
          content: response.message,
          meta: `${response.model} · ${response.promptProfile}`,
        },
        ],
      );
    } catch (error) {
      const message = (error as Error).message;
      toast.error(`请求失败: ${message}`);
      replaceConversationMessages(
        targetConversationId,
        [
          ...optimisticMessages,
        {
          id: nextMessageId(),
          role: 'system',
          content: message,
        },
        ],
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <>
      <title>模型对话 - Liteyuki WebUI</title>
      <div className='flex h-[calc(100vh-4rem)] flex-col overflow-hidden p-2 md:p-4'>
        <div className='mx-auto flex h-full w-full max-w-6xl min-h-0 flex-col gap-3'>
          <div
          className={clsx(
              'flex flex-wrap items-center justify-between gap-3 rounded-[28px] border px-4 py-3 backdrop-blur-xl',
              themePanelClass
            )}
          >
            <div className='min-w-0'>
              <div className='text-sm font-semibold'>模型对话</div>
              <div className={clsx('text-xs', mutedTextClass)}>
                Ask LiteyukiBot
                {settings?.promptProfile ? ` · 配置 ${settings.promptProfile}` : ''}
              </div>
            </div>

            <div className='flex items-center gap-2'>
              <Chip
                size='sm'
                variant='flat'
                color={statusTone}
                className='backdrop-blur-sm'
              >
                {statusText}
              </Chip>
              <Button
                isIconOnly
                radius='full'
                variant='light'
                aria-label='新建对话'
                onPress={handleCreateConversation}
              >
                <LuPlus />
              </Button>
              <Button
                isIconOnly
                radius='full'
                variant='light'
                onPress={() => refreshSettings()}
                isLoading={settingsLoading}
              >
                <LuRefreshCw />
              </Button>
              <Button
                isIconOnly
                radius='full'
                variant='light'
                aria-label='删除当前对话'
                onPress={() => handleDeleteConversation(activeConversationId)}
              >
                <LuTrash2 />
              </Button>
            </div>
          </div>

          <div
            className={clsx(
              'flex min-h-0 flex-1 flex-col overflow-hidden rounded-[32px] border backdrop-blur-xl',
              themeSurfaceClass
            )}
          >
            <div className='flex min-h-0 flex-1 flex-col md:flex-row'>
              <aside
                className={clsx(
                  'shrink-0 border-b p-3 md:w-64 md:border-b-0 md:border-r',
                  themeSubtleSurfaceClass
                )}
              >
                <div className='mb-3 flex items-center justify-between gap-2'>
                  <div className={clsx('text-xs font-semibold uppercase tracking-[0.18em]', quietTextClass)}>
                    对话
                  </div>
                  <Button
                    isIconOnly
                    size='sm'
                    radius='full'
                    color='primary'
                    variant='flat'
                    aria-label='新建对话'
                    onPress={handleCreateConversation}
                  >
                    <LuPlus />
                  </Button>
                </div>

                <div className='flex gap-2 overflow-x-auto pb-1 md:max-h-[calc(100vh-17rem)] md:flex-col md:overflow-y-auto md:overflow-x-hidden md:pb-0'>
                  {conversationState.conversations.map((conversation) => {
                    const isActive = conversation.id === activeConversationId;

                    return (
                      <div
                        key={conversation.id}
                        className={clsx(
                          'group flex min-w-56 items-center gap-2 rounded-2xl border p-2 transition md:min-w-0',
                          isActive
                            ? hasBackground
                              ? isDark
                                ? 'border-white/20 bg-white/12 shadow-sm'
                                : 'border-black/10 bg-white/72 shadow-sm'
                              : 'border-primary/20 bg-primary/10 shadow-sm'
                            : hasBackground
                              ? isDark
                                ? 'border-white/10 bg-white/5 hover:bg-white/10'
                                : 'border-black/8 bg-white/42 hover:bg-white/60'
                              : 'border-white/30 bg-white/45 hover:bg-white/75 dark:border-white/10 dark:bg-white/5 dark:hover:bg-white/10'
                        )}
                      >
                        <button
                          type='button'
                          className='min-w-0 flex-1 text-left'
                          onClick={() => handleSwitchConversation(conversation.id)}
                        >
                          <div className={clsx(
                            'truncate text-sm font-semibold',
                            strongTextClass
                          )}
                          >
                            {conversation.title}
                          </div>
                          <div className={clsx(
                            'mt-1 flex items-center gap-2 truncate text-xs',
                            quietTextClass
                          )}
                          >
                            <span>{conversation.messages.length} 条</span>
                            <span>·</span>
                            <span>{formatConversationTime(conversation.updatedAt)}</span>
                          </div>
                          <div className={clsx(
                            'mt-1 truncate text-xs',
                            faintTextClass
                          )}
                          >
                            {conversation.model || currentModel || '未选择模型'}
                          </div>
                        </button>
                        <Button
                          isIconOnly
                          size='sm'
                          radius='full'
                          variant='light'
                          aria-label='删除对话'
                          className={clsx(
                            'shrink-0 opacity-70 transition group-hover:opacity-100',
                            isActive && 'opacity-100'
                          )}
                          onPress={() => handleDeleteConversation(conversation.id)}
                        >
                          <LuTrash2 />
                        </Button>
                      </div>
                    );
                  })}
                </div>
              </aside>

              <div className='flex min-h-0 flex-1 flex-col'>
                <div ref={listRef} className='flex-1 overflow-y-auto px-3 py-4 md:px-6 md:py-6'>
              {messages.length === 0
                ? (
                  <div className='flex h-full items-center justify-center'>
                    <div className='w-full max-w-2xl px-4 text-center'>
                      <div className={clsx(
                        'mb-4 text-3xl font-semibold tracking-tight md:text-4xl',
                        strongTextClass
                      )}
                      >
                        Ask LiteyukiBot
                      </div>
                      <p className={clsx(
                        'mx-auto max-w-xl text-sm leading-7 md:text-base',
                        mutedTextClass
                      )}
                      >
                        从这里开始一轮新的对话。
                      </p>
                      <div className='mt-5 flex flex-wrap items-center justify-center gap-2'>
                        <Chip size='sm' variant='flat' color='primary'>
                          {currentProviderLabel}
                        </Chip>
                        <Chip size='sm' variant='flat' color='default'>
                          {currentModel || '未选择模型'}
                        </Chip>
                        {supports.imageInput && (
                          <Chip size='sm' variant='flat' color='secondary'>
                            支持图片输入
                          </Chip>
                        )}
                      </div>
                    </div>
                  </div>
                  )
                : (
                  <div className='space-y-4'>
                    {messages.map((message) => (
                      <MessageBubble
                        assistantBubbleClass={assistantBubbleClass}
                        key={message.id}
                        hasBackground={hasBackground}
                        message={message}
                      />
                    ))}
                  </div>
                  )}
            </div>

            <div className={clsx(
              'border-t p-3 md:p-4',
              themeSubtleSurfaceClass
            )}
            >
              <div className='mx-auto w-full max-w-4xl'>
                {attachments.length > 0 && (
                  <div className='mb-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3'>
                    {attachments.map((attachment, index) => (
                      <AttachmentCard
                        key={`${attachment.name}-${index}`}
                        attachment={attachment}
                        onRemove={() => {
                          setAttachments((current) => current.filter((_, currentIndex) => currentIndex !== index));
                        }}
                      />
                    ))}
                  </div>
                )}

                <div className='mb-3 flex flex-wrap items-center gap-2'>
                  <Chip size='sm' variant='flat' color='primary'>
                    {currentProviderLabel}
                  </Chip>
                  <Chip size='sm' variant='flat' color='default'>
                    {currentModel || '未选择模型'}
                  </Chip>
                  {supports.reasoningEffort && config.reasoningEffort && config.reasoningEffort !== 'none' && (
                    <Chip size='sm' variant='flat' color='secondary'>
                      推理 {config.reasoningEffort}
                    </Chip>
                  )}
                </div>

                <div className={clsx(
                  'rounded-[30px] border p-2 shadow-sm',
                  composerShellClass
                )}
                >
                  <Textarea
                    minRows={1}
                    maxRows={7}
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    onPaste={handlePaste}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
                        event.preventDefault();
                        if (canSend) {
                          void handleSend();
                        }
                      }
                    }}
                    placeholder='Ask LiteyukiBot'
                    variant='flat'
                    classNames={{
                      inputWrapper: 'border-none bg-transparent shadow-none px-2 py-1 data-[hover=true]:bg-transparent group-data-[focus=true]:bg-transparent',
                      input: clsx('resize-none text-sm md:text-base', composerInputClass),
                    }}
                  />

                  <div className='mt-2 flex items-center justify-between gap-2'>
                    <div className='flex items-center gap-2'>
                      <Button
                        isIconOnly
                        radius='full'
                        variant='flat'
                        onPress={() => fileInputRef.current?.click()}
                      >
                        <LuPlus />
                      </Button>
                      <Button
                        isIconOnly
                        radius='full'
                        variant='light'
                        onPress={() => setSettingsOpen(true)}
                      >
                        <LuSettings2 />
                      </Button>
                    </div>

                    <Button
                      color='primary'
                      radius='full'
                      className='min-w-[120px] font-semibold'
                      onPress={() => void handleSend()}
                      isLoading={submitting}
                      isDisabled={!canSend}
                      startContent={!submitting ? <LuSend /> : undefined}
                    >
                      发送
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
        </div>
        </div>

        <input
          ref={fileInputRef}
          type='file'
          hidden
          multiple
          accept={acceptedAttachments}
          onChange={(event) => {
            const files = Array.from(event.target.files ?? []);
            void appendFiles(files);
            event.target.value = '';
          }}
        />

        <Modal
          isOpen={settingsOpen}
          onOpenChange={setSettingsOpen}
          size='2xl'
          backdrop='blur'
          scrollBehavior='inside'
        >
          <ModalContent>
            {(onClose) => (
              <>
                <ModalHeader>模型设置</ModalHeader>
                <ModalBody className='gap-4'>
                  {providerOptions.length > 0
                    ? (
                      <Select
                        label='供应商'
                        selectedKeys={currentBaseUrl ? [currentBaseUrl] : []}
                        onSelectionChange={(keys) => {
                          const selected = Array.from(keys)[0];
                          if (typeof selected === 'string') {
                            setConfigField('baseUrl', selected);
                            const selectedOption = providerOptions.find((option) => option.baseUrl === selected);
                            const nextProviderId = detectProviderId(selected);
                            const nextModels = selectedOption?.modelOptions?.length
                              ? selectedOption.modelOptions
                              : modelOptionsForProvider(nextProviderId, providerCatalog);
                            if (nextModels.length > 0 && !nextModels.includes(currentModel)) {
                              setConfigField('model', nextModels[0]);
                            }
                          }
                        }}
                        variant='bordered'
                      >
                        {providerOptions.map((option) => (
                          <SelectItem key={option.baseUrl} textValue={option.label}>
                            {option.label}
                          </SelectItem>
                        ))}
                      </Select>
                      )
                    : (
                      <Input
                        label='供应商地址'
                        value={config.baseUrl}
                        onChange={(event) => setConfigField('baseUrl', event.target.value)}
                        placeholder='https://api.openai.com/v1'
                        variant='bordered'
                      />
                      )}

                  {modelOptions.length > 0
                    ? (
                      <Select
                        label='模型'
                        selectedKeys={currentModel ? [currentModel] : []}
                        onSelectionChange={(keys) => {
                          const selected = Array.from(keys)[0];
                          if (typeof selected === 'string') {
                            setConfigField('model', selected);
                          }
                        }}
                        variant='bordered'
                      >
                        {modelOptions.map((option) => (
                          <SelectItem key={option} textValue={option}>
                            {option}
                          </SelectItem>
                        ))}
                      </Select>
                      )
                    : (
                      <Input
                        label='模型'
                        value={config.model}
                        onChange={(event) => setConfigField('model', event.target.value)}
                        placeholder='例如 gpt-4.1-mini / gpt-5-mini'
                        variant='bordered'
                      />
                      )}

                  {supports.reasoningEffort && reasoningOptions.length > 0 && (
                    <Select
                      label='推理强度'
                      selectedKeys={config.reasoningEffort ? [config.reasoningEffort] : []}
                      onSelectionChange={(keys) => {
                        const selected = Array.from(keys)[0];
                        if (typeof selected === 'string') {
                          setConfigField('reasoningEffort', selected);
                        }
                      }}
                      variant='bordered'
                    >
                      {reasoningOptions.map((option) => (
                        <SelectItem key={option} textValue={option}>
                          {option}
                        </SelectItem>
                      ))}
                    </Select>
                  )}

                  <div className='grid gap-4 md:grid-cols-3 xl:grid-cols-5'>
                    {supports.temperature && (
                      <Input
                        label='Temperature'
                        value={config.temperature}
                        onChange={(event) => setConfigField('temperature', event.target.value)}
                        placeholder='默认'
                        variant='bordered'
                      />
                    )}
                    {supports.topP && (
                      <Input
                        label='Top P'
                        value={config.topP}
                        onChange={(event) => setConfigField('topP', event.target.value)}
                        placeholder='默认'
                        variant='bordered'
                      />
                    )}
                    {supports.topK && (
                      <Input
                        label='Top K'
                        value={config.topK}
                        onChange={(event) => setConfigField('topK', event.target.value)}
                        placeholder='默认'
                        variant='bordered'
                      />
                    )}
                    {supports.frequencyPenalty && (
                      <Input
                        label='Frequency Penalty'
                        value={config.frequencyPenalty}
                        onChange={(event) => setConfigField('frequencyPenalty', event.target.value)}
                        placeholder='默认'
                        variant='bordered'
                      />
                    )}
                    {supports.presencePenalty && (
                      <Input
                        label='Presence Penalty'
                        value={config.presencePenalty}
                        onChange={(event) => setConfigField('presencePenalty', event.target.value)}
                        placeholder='默认'
                        variant='bordered'
                      />
                    )}
                  </div>
                </ModalBody>
                <ModalFooter>
                  <Button variant='light' radius='full' onPress={handleResetConfig}>
                    重置
                  </Button>
                  <Button variant='light' radius='full' onPress={() => refreshSettings()}>
                    刷新配置
                  </Button>
                  <Button color='primary' radius='full' onPress={onClose}>
                    完成
                  </Button>
                </ModalFooter>
              </>
            )}
          </ModalContent>
        </Modal>
      </div>
    </>
  );
}
