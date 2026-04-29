import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import { Input, Textarea } from '@heroui/input';
import { Select, SelectItem } from '@heroui/select';
import { Switch } from '@heroui/switch';
import { useLocalStorage } from '@uidotdev/usehooks';
import clsx from 'clsx';
import { useEffect, useMemo, useState } from 'react';
import toast from 'react-hot-toast';
import {
  LuCircleCheck,
  LuCircleX,
  LuCopy,
  LuFlaskConical,
  LuLoaderCircle,
  LuPlus,
  LuRefreshCw,
  LuSave,
  LuTrash2,
  LuWandSparkles,
} from 'react-icons/lu';

import key from '@/const/key';
import LlmManager, {
  type LlmManagedProvider,
  type LlmManagerPreviewResponse,
  type LlmManagerState,
  type LlmModelTestResult,
  type LlmProviderCatalogItem,
} from '@/controllers/llm_manager';

const DEFAULT_TIMEOUT_SECONDS = 120;

function headersToText (headers: Record<string, string>) {
  return Object.entries(headers)
    .map(([headerKey, value]) => `${headerKey}: ${value}`)
    .join('\n');
}

function parseHeadersText (raw: string) {
  const entries = raw
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const separatorIndex = line.indexOf(':');
      if (separatorIndex < 0) {
        return null;
      }
      const headerKey = line.slice(0, separatorIndex).trim();
      const value = line.slice(separatorIndex + 1).trim();
      if (!headerKey || !value) {
        return null;
      }
      return [headerKey, value] as const;
    })
    .filter((entry): entry is readonly [string, string] => entry !== null);

  return Object.fromEntries(entries);
}

function normalizeProviderId (input: string) {
  const normalized = input
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
  return normalized || `provider-${Date.now()}`;
}

function getProviderCatalogItem (
  providerId: string | undefined,
  catalog: LlmProviderCatalogItem[]
) {
  if (!providerId) {
    return undefined;
  }
  return catalog.find((item) => item.id === providerId);
}

function getDefaultBaseUrl (catalogItem?: LlmProviderCatalogItem) {
  return catalogItem?.baseUrls.find((item) => item.default)?.url
    ?? catalogItem?.baseUrls[0]?.url
    ?? '';
}

function applyCatalogMetadata (
  provider: LlmManagedProvider,
  catalog: LlmProviderCatalogItem[]
) {
  const catalogItem = getProviderCatalogItem(provider.providerId, catalog);
  const fallbackBaseUrl = getDefaultBaseUrl(catalogItem);
  return {
    ...provider,
    providerId: catalogItem?.id ?? provider.providerId ?? 'openai-compatible',
    providerLabel: catalogItem?.label ?? provider.providerLabel ?? provider.label,
    baseUrl: provider.baseUrl.trim() || fallbackBaseUrl,
  };
}

function buildNewProvider (
  catalog: LlmProviderCatalogItem[],
  index: number
): LlmManagedProvider {
  const firstCatalog = catalog[0];
  const defaultBaseUrl = getDefaultBaseUrl(firstCatalog);
  const defaultProviderId = firstCatalog?.id ?? 'openai-compatible';
  const defaultLabel = firstCatalog?.label ?? `Provider ${index}`;
  const id = normalizeProviderId(defaultLabel);

  return {
    id,
    label: defaultLabel,
    providerId: defaultProviderId,
    providerLabel: firstCatalog?.label ?? defaultLabel,
    baseUrl: defaultBaseUrl,
    apiKey: '',
    timeoutSeconds: DEFAULT_TIMEOUT_SECONDS,
    headers: {},
    models: [],
    active: false,
  };
}

function normalizeManagerState (
  state: LlmManagerState,
  catalog: LlmProviderCatalogItem[]
) {
  const providers = (state.providers.length > 0
    ? state.providers
    : [buildNewProvider(catalog, 1)])
    .map((provider) => applyCatalogMetadata(provider, catalog));
  const nextActiveProviderId = providers.some((provider) => provider.id === state.activeProviderId)
    ? state.activeProviderId
    : providers[0]?.id;

  return {
    providers,
    activeProviderId: nextActiveProviderId,
    configPath: state.configPath,
    providerCatalog: state.providerCatalog,
  };
}

function buildProviderPayload (
  provider: LlmManagedProvider,
  catalog: LlmProviderCatalogItem[]
) {
  return applyCatalogMetadata(provider, catalog);
}

function formatLatency (result: LlmModelTestResult) {
  return `${result.modelId} ${result.latencyMs}ms`;
}

export default function LlmManagerPage () {
  const [backgroundImage] = useLocalStorage<string>(key.backgroundImage, '');
  const hasBackground = !!backgroundImage;

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [testingAll, setTestingAll] = useState(false);
  const [previewing, setPreviewing] = useState(false);

  const [providerCatalog, setProviderCatalog] = useState<LlmProviderCatalogItem[]>([]);
  const [configPath, setConfigPath] = useState('');
  const [providers, setProviders] = useState<LlmManagedProvider[]>([]);
  const [activeProviderId, setActiveProviderId] = useState<string>();
  const [selectedProviderId, setSelectedProviderId] = useState<string>();
  const [headersDraft, setHeadersDraft] = useState('');
  const [newModelId, setNewModelId] = useState('');
  const [previewModelId, setPreviewModelId] = useState('');
  const [previewData, setPreviewData] = useState<LlmManagerPreviewResponse | null>(null);
  const [runningModelId, setRunningModelId] = useState<string>();

  const activeProvider = useMemo(() => {
    return providers.find((provider) => provider.id === activeProviderId);
  }, [activeProviderId, providers]);

  const selectedProvider = useMemo(() => {
    return providers.find((provider) => provider.id === selectedProviderId)
      ?? activeProvider
      ?? providers[0];
  }, [activeProvider, providers, selectedProviderId]);

  const selectedCatalogProvider = useMemo(() => {
    return getProviderCatalogItem(selectedProvider?.providerId, providerCatalog);
  }, [providerCatalog, selectedProvider?.providerId]);

  useEffect(() => {
    if (!selectedProvider) {
      setHeadersDraft('');
      return;
    }
    setHeadersDraft(headersToText(selectedProvider.headers));
  }, [selectedProvider?.id, selectedProvider?.headers]);

  useEffect(() => {
    if (!selectedProvider) {
      setPreviewModelId('');
      return;
    }
    const selectedModel = selectedProvider.models.find((model) => model.enabled)?.id
      ?? selectedProvider.models[0]?.id
      ?? '';
    setPreviewModelId((current) => {
      if (current && selectedProvider.models.some((model) => model.id === current)) {
        return current;
      }
      return selectedModel;
    });
  }, [selectedProvider?.id, selectedProvider?.models]);

  const loadManagerState = async () => {
    setLoading(true);
    try {
      const state = await LlmManager.getManagerState();
      const normalized = normalizeManagerState(state, state.providerCatalog);
      setProviderCatalog(normalized.providerCatalog);
      setConfigPath(normalized.configPath);
      setProviders(normalized.providers);
      setActiveProviderId(normalized.activeProviderId);
      setSelectedProviderId(normalized.activeProviderId ?? normalized.providers[0]?.id);
      setPreviewData(null);
      setNewModelId('');
    } catch (error) {
      toast.error(`加载模型管理状态失败: ${(error as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadManagerState();
  }, []);

  const replaceProvider = (
    providerId: string,
    updater: (provider: LlmManagedProvider) => LlmManagedProvider
  ) => {
    setProviders((current) => current.map((provider) => {
      if (provider.id !== providerId) {
        return provider;
      }
      return updater(provider);
    }));
  };

  const applyHeadersDraft = (baseProviders: LlmManagedProvider[]) => {
    if (!selectedProvider) {
      return baseProviders;
    }
    const nextHeaders = parseHeadersText(headersDraft);
    return baseProviders.map((provider) => {
      if (provider.id !== selectedProvider.id) {
        return provider;
      }
      return {
        ...provider,
        headers: nextHeaders,
      };
    });
  };

  const updateSelectedProvider = (updater: (provider: LlmManagedProvider) => LlmManagedProvider) => {
    if (!selectedProvider) {
      return;
    }
    replaceProvider(selectedProvider.id, updater);
  };

  const handleAddProvider = () => {
    const candidate = buildNewProvider(providerCatalog, providers.length + 1);
    let nextId = candidate.id;
    let counter = 1;
    while (providers.some((provider) => provider.id === nextId)) {
      counter += 1;
      nextId = `${candidate.id}-${counter}`;
    }
    const providerToInsert = {
      ...candidate,
      id: nextId,
      active: providers.length === 0,
    };
    const nextProviders = [...providers, providerToInsert];
    setProviders(nextProviders);
    if (!activeProviderId) {
      setActiveProviderId(providerToInsert.id);
    }
    setSelectedProviderId(providerToInsert.id);
  };

  const handleDeleteProvider = (providerId: string) => {
    const nextProviders = providers.filter((provider) => provider.id !== providerId);
    if (nextProviders.length === 0) {
      const fallback = buildNewProvider(providerCatalog, 1);
      setProviders([{ ...fallback, active: true }]);
      setActiveProviderId(fallback.id);
      setSelectedProviderId(fallback.id);
      return;
    }
    setProviders(nextProviders);
    if (activeProviderId === providerId) {
      setActiveProviderId(nextProviders[0].id);
    }
    if (selectedProviderId === providerId) {
      setSelectedProviderId(nextProviders[0].id);
    }
  };

  const handleProviderIdChange = (nextIdInput: string) => {
    if (!selectedProvider) {
      return;
    }
    const nextId = normalizeProviderId(nextIdInput);
    if (nextId === selectedProvider.id) {
      return;
    }
    if (providers.some((provider) => provider.id === nextId)) {
      toast.error('provider id 已存在');
      return;
    }

    const previousId = selectedProvider.id;
    replaceProvider(previousId, (provider) => ({
      ...provider,
      id: nextId,
    }));
    if (activeProviderId === previousId) {
      setActiveProviderId(nextId);
    }
    if (selectedProviderId === previousId) {
      setSelectedProviderId(nextId);
    }
  };

  const persistManagerState = async (
    nextActiveProviderId: string | undefined,
    nextProviders: LlmManagedProvider[],
    successMessage: string
  ) => {
    const providersToSave = applyHeadersDraft(nextProviders).map((provider) => (
      buildProviderPayload(provider, providerCatalog)
    ));
    setProviders(providersToSave);
    setSaving(true);
    try {
      const state = await LlmManager.saveManagerState({
        activeProviderId: nextActiveProviderId,
        providers: providersToSave,
      });
      const normalized = normalizeManagerState(state, state.providerCatalog);
      setProviderCatalog(normalized.providerCatalog);
      setConfigPath(normalized.configPath);
      setProviders(normalized.providers);
      setActiveProviderId(normalized.activeProviderId);
      setSelectedProviderId((current) => {
        if (current && normalized.providers.some((provider) => provider.id === current)) {
          return current;
        }
        return normalized.activeProviderId ?? normalized.providers[0]?.id;
      });
      toast.success(successMessage);
    } catch (error) {
      toast.error(`保存失败: ${(error as Error).message}`);
      await loadManagerState();
    } finally {
      setSaving(false);
    }
  };

  const handleSave = async () => {
    await persistManagerState(activeProviderId, providers, '模型管理配置已保存');
  };

  const handleSetActiveProvider = async (providerId: string) => {
    if (providerId === activeProviderId || saving) {
      return;
    }
    setActiveProviderId(providerId);
    setSelectedProviderId(providerId);
    await persistManagerState(providerId, providers, 'Active Provider 已切换');
  };

  const handleFetchModels = async () => {
    if (!selectedProvider) {
      return;
    }
    const providerForRequest = buildProviderPayload({
      ...selectedProvider,
      headers: parseHeadersText(headersDraft),
    }, providerCatalog);
    setFetchingModels(true);
    try {
      const result = await LlmManager.fetchModels(providerForRequest);
      replaceProvider(selectedProvider.id, (provider) => ({
        ...provider,
        providerId: result.provider.providerId,
        providerLabel: result.provider.providerLabel,
        models: result.models,
      }));
      toast.success(`模型列表已更新 (${result.source})`);
    } catch (error) {
      toast.error(`获取模型失败: ${(error as Error).message}`);
    } finally {
      setFetchingModels(false);
    }
  };

  const handleToggleModel = (modelId: string, enabled: boolean) => {
    updateSelectedProvider((provider) => ({
      ...provider,
      models: provider.models.map((model) => {
        if (model.id !== modelId) {
          return model;
        }
        return {
          ...model,
          enabled,
        };
      }),
    }));
  };

  const handleAddModel = () => {
    const modelId = newModelId.trim();
    if (!modelId || !selectedProvider) {
      return;
    }
    if (selectedProvider.models.some((model) => model.id === modelId)) {
      toast.error('该模型已存在');
      return;
    }
    updateSelectedProvider((provider) => ({
      ...provider,
      models: [...provider.models, { id: modelId, enabled: provider.models.length === 0 }],
    }));
    setNewModelId('');
    if (!previewModelId) {
      setPreviewModelId(modelId);
    }
  };

  const handleRemoveModel = (modelId: string) => {
    if (!selectedProvider) {
      return;
    }
    updateSelectedProvider((provider) => ({
      ...provider,
      models: provider.models.filter((model) => model.id !== modelId),
    }));
    if (previewModelId === modelId) {
      setPreviewModelId('');
    }
  };

  const showTestResultToast = (result: LlmModelTestResult) => {
    if (result.ok) {
      toast.success(`测试通过: ${formatLatency(result)}`);
      return;
    }
    toast.error(`测试失败: ${formatLatency(result)}${result.error ? ` · ${result.error}` : ''}`);
  };

  const handleTestSingle = async (modelId: string) => {
    if (!selectedProvider) {
      return;
    }
    const providerForRequest = buildProviderPayload({
      ...selectedProvider,
      headers: parseHeadersText(headersDraft),
    }, providerCatalog);
    setRunningModelId(modelId);
    try {
      const result = await LlmManager.testModels(providerForRequest, { modelId });
      const first = result.results[0];
      if (first) {
        showTestResultToast(first);
      } else {
        toast.error('测试返回为空');
      }
    } catch (error) {
      toast.error(`单模型测试失败: ${(error as Error).message}`);
    } finally {
      setRunningModelId(undefined);
    }
  };

  const handleTestAll = async () => {
    if (!selectedProvider) {
      return;
    }
    const providerForRequest = buildProviderPayload({
      ...selectedProvider,
      headers: parseHeadersText(headersDraft),
    }, providerCatalog);
    setTestingAll(true);
    try {
      const result = await LlmManager.testModels(providerForRequest, { testAll: true });
      if (result.results.length === 0) {
        toast.error('没有可测试模型');
        return;
      }
      result.results.forEach(showTestResultToast);
    } catch (error) {
      toast.error(`批量测试失败: ${(error as Error).message}`);
    } finally {
      setTestingAll(false);
    }
  };

  const handlePreview = async () => {
    if (!selectedProvider) {
      return;
    }
    const providerForRequest = buildProviderPayload({
      ...selectedProvider,
      headers: parseHeadersText(headersDraft),
    }, providerCatalog);
    setPreviewing(true);
    try {
      const response = await LlmManager.previewRequest(providerForRequest, previewModelId || undefined);
      setPreviewData(response);
      toast.success('请求预览已生成');
    } catch (error) {
      toast.error(`请求预览失败: ${(error as Error).message}`);
    } finally {
      setPreviewing(false);
    }
  };

  const activeProviderCount = providers.filter((provider) => provider.id === activeProviderId).length;

  return (
    <>
      <title>模型管理 - Liteyuki WebUI</title>
      <div className='h-[calc(100vh-3.5rem)] px-2 py-2 md:px-4'>
        <div className='h-full overflow-hidden rounded-2xl border border-white/20 bg-white/55 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
          <div className='flex h-full min-h-0 flex-col md:flex-row'>
            <aside className={clsx(
              'w-full shrink-0 border-b p-3 md:w-72 md:border-b-0 md:border-r',
              hasBackground
                ? 'border-white/20 bg-white/10'
                : 'border-white/25 bg-white/45 dark:bg-white/5'
            )}
            >
              <div className='mb-3 flex items-center justify-between'>
                <div>
                  <div className='text-sm font-semibold text-default-700 dark:text-default-100'>
                    Provider
                  </div>
                  <div className='text-xs text-default-400'>
                    Active: {activeProviderCount > 0 ? activeProviderId : '未设置'}
                  </div>
                </div>
                <Button isIconOnly size='sm' color='primary' variant='flat' onPress={handleAddProvider}>
                  <LuPlus />
                </Button>
              </div>

              <div className='space-y-2 overflow-y-auto md:max-h-[calc(100vh-11rem)]'>
                {providers.map((provider) => {
                  const isSelected = provider.id === selectedProvider?.id;
                  const isActive = provider.id === activeProviderId;
                  return (
                    <div
                      key={provider.id}
                      className={clsx(
                        'rounded-xl border p-2 transition',
                        isSelected
                          ? 'border-primary/40 bg-primary/10'
                          : 'border-white/20 bg-white/40 hover:bg-white/65 dark:border-white/10 dark:bg-white/5 dark:hover:bg-white/10'
                      )}
                    >
                      <button
                        type='button'
                        className='w-full text-left'
                        onClick={() => setSelectedProviderId(provider.id)}
                      >
                        <div className='truncate text-sm font-semibold text-default-700 dark:text-default-100'>
                          {provider.label || provider.id}
                        </div>
                        <div className='truncate text-xs text-default-400'>
                          {provider.baseUrl || '未设置 baseUrl'}
                        </div>
                      </button>
                      <div className='mt-2 flex items-center justify-between'>
                        {isActive
                          ? (
                            <Chip size='sm' color='primary' variant='flat'>
                              Active
                            </Chip>
                            )
                          : (
                            <Button
                              size='sm'
                              variant='flat'
                              color='default'
                              onPress={() => void handleSetActiveProvider(provider.id)}
                              isLoading={saving && provider.id === selectedProviderId}
                            >
                              设为 Active
                            </Button>
                            )}
                        <Button
                          isIconOnly
                          size='sm'
                          color='danger'
                          variant='light'
                          onPress={() => handleDeleteProvider(provider.id)}
                        >
                          <LuTrash2 />
                        </Button>
                      </div>
                    </div>
                  );
                })}
              </div>
            </aside>

            <section className='flex min-h-0 flex-1 flex-col'>
              <div className='border-b border-white/20 p-3 dark:border-white/10'>
                <div className='flex flex-wrap items-center justify-between gap-2'>
                  <div>
                    <div className='text-base font-semibold text-default-700 dark:text-default-100'>模型管理</div>
                    <div className='text-xs text-default-400'>配置路径: {configPath || '-'}</div>
                  </div>
                  <div className='flex items-center gap-2'>
                    <Button
                      variant='flat'
                      startContent={<LuRefreshCw />}
                      onPress={() => void loadManagerState()}
                      isLoading={loading}
                    >
                      刷新
                    </Button>
                    <Button
                      color='primary'
                      startContent={<LuSave />}
                      onPress={() => void handleSave()}
                      isLoading={saving}
                    >
                      保存
                    </Button>
                  </div>
                </div>
              </div>

              <div className='grid min-h-0 flex-1 grid-cols-1 gap-3 overflow-y-auto p-3 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]'>
                <div className='space-y-3'>
                  <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                    <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>Provider 配置</div>
                    {!selectedProvider
                      ? (
                        <div className='text-sm text-default-400'>暂无 provider</div>
                        )
                      : (
                        <div className='grid gap-3'>
                          <Input
                            label='id'
                            value={selectedProvider.id}
                            onChange={(event) => handleProviderIdChange(event.target.value)}
                            variant='bordered'
                          />
                          <Input
                            label='label'
                            value={selectedProvider.label}
                            onChange={(event) => updateSelectedProvider((provider) => ({
                              ...provider,
                              label: event.target.value,
                            }))}
                            variant='bordered'
                          />
                          <Select
                            label='供应商'
                            variant='bordered'
                            selectedKeys={selectedProvider.providerId ? [selectedProvider.providerId] : []}
                            onSelectionChange={(keys) => {
                              const nextProviderId = Array.from(keys)[0];
                              if (typeof nextProviderId !== 'string') {
                                return;
                              }
                              const catalogItem = getProviderCatalogItem(nextProviderId, providerCatalog);
                              updateSelectedProvider((provider) => ({
                                ...provider,
                                providerId: nextProviderId,
                                providerLabel: catalogItem?.label ?? provider.providerLabel,
                                baseUrl: getDefaultBaseUrl(catalogItem),
                              }));
                            }}
                          >
                            {providerCatalog.map((item) => (
                              <SelectItem key={item.id}>{item.label}</SelectItem>
                            ))}
                          </Select>
                          <Select
                            label='端点预设'
                            variant='bordered'
                            placeholder='可手动覆盖为自定义地址'
                            selectedKeys={
                              selectedCatalogProvider?.baseUrls.some((item) => item.url === selectedProvider.baseUrl)
                                ? [selectedProvider.baseUrl]
                                : []
                            }
                            onSelectionChange={(keys) => {
                              const nextBaseUrl = Array.from(keys)[0];
                              if (typeof nextBaseUrl !== 'string') {
                                return;
                              }
                              updateSelectedProvider((provider) => ({
                                ...provider,
                                baseUrl: nextBaseUrl,
                              }));
                            }}
                          >
                            {(selectedCatalogProvider?.baseUrls ?? []).map((item) => (
                              <SelectItem key={item.url}>
                                {item.region ? `${item.label} (${item.region})` : item.label}
                              </SelectItem>
                            ))}
                          </Select>
                          <Input
                            label='baseUrl'
                            value={selectedProvider.baseUrl}
                            onChange={(event) => updateSelectedProvider((provider) => ({
                              ...provider,
                              baseUrl: event.target.value,
                            }))}
                            variant='bordered'
                          />
                          <Input
                            label='apiKey'
                            value={selectedProvider.apiKey}
                            onChange={(event) => updateSelectedProvider((provider) => ({
                              ...provider,
                              apiKey: event.target.value,
                            }))}
                            variant='bordered'
                            type='password'
                          />
                          <Input
                            label='timeoutSeconds'
                            value={String(selectedProvider.timeoutSeconds || DEFAULT_TIMEOUT_SECONDS)}
                            onChange={(event) => {
                              const parsed = Number.parseInt(event.target.value, 10);
                              updateSelectedProvider((provider) => ({
                                ...provider,
                                timeoutSeconds: Number.isFinite(parsed) && parsed > 0
                                  ? parsed
                                  : DEFAULT_TIMEOUT_SECONDS,
                              }));
                            }}
                            variant='bordered'
                            type='number'
                            min={1}
                          />
                          <Textarea
                            label='headers'
                            value={headersDraft}
                            onChange={(event) => setHeadersDraft(event.target.value)}
                            onBlur={() => {
                              if (!selectedProvider) {
                                return;
                              }
                              const parsed = parseHeadersText(headersDraft);
                              updateSelectedProvider((provider) => ({
                                ...provider,
                                headers: parsed,
                              }));
                            }}
                            minRows={3}
                            variant='bordered'
                            placeholder='Header-Name: value'
                          />
                          <div className='text-xs text-default-400'>
                            providerId: {selectedProvider.providerId}
                          </div>
                        </div>
                        )}
                  </div>

                  <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                    <div className='mb-2 flex items-center justify-between'>
                      <div className='text-sm font-semibold text-default-700 dark:text-default-100'>模型列表</div>
                      <div className='flex items-center gap-2'>
                        <Button
                          size='sm'
                          variant='flat'
                          startContent={fetchingModels ? <LuLoaderCircle className='animate-spin' /> : <LuWandSparkles />}
                          isLoading={fetchingModels}
                          onPress={() => void handleFetchModels()}
                        >
                          获取模型
                        </Button>
                        <Button
                          size='sm'
                          variant='flat'
                          color='secondary'
                          startContent={<LuFlaskConical />}
                          isLoading={testingAll}
                          onPress={() => void handleTestAll()}
                        >
                          批量测试
                        </Button>
                      </div>
                    </div>

                    <div className='mb-3 flex items-center gap-2'>
                      <Input
                        value={newModelId}
                        onChange={(event) => setNewModelId(event.target.value)}
                        placeholder='输入模型 ID'
                        variant='bordered'
                        size='sm'
                      />
                      <Button size='sm' color='primary' variant='flat' onPress={handleAddModel}>
                        添加
                      </Button>
                    </div>

                    <div className='space-y-2'>
                      {selectedProvider?.models.length
                        ? selectedProvider.models.map((model) => (
                          <div
                            key={model.id}
                            className='flex items-center justify-between rounded-lg border border-white/20 bg-white/50 px-3 py-2 dark:border-white/10 dark:bg-white/5'
                          >
                            <div className='min-w-0 flex-1 pr-3'>
                              <div className='truncate text-sm font-medium text-default-700 dark:text-default-100'>
                                {model.id}
                              </div>
                            </div>
                            <div className='flex items-center gap-2'>
                              <Switch
                                isSelected={model.enabled}
                                onValueChange={(value) => handleToggleModel(model.id, value)}
                                size='sm'
                              />
                              <Button
                                size='sm'
                                variant='flat'
                                isLoading={runningModelId === model.id}
                                onPress={() => void handleTestSingle(model.id)}
                                startContent={runningModelId === model.id ? undefined : <LuFlaskConical />}
                              >
                                单测
                              </Button>
                              <Button
                                isIconOnly
                                size='sm'
                                color='danger'
                                variant='light'
                                onPress={() => handleRemoveModel(model.id)}
                              >
                                <LuTrash2 />
                              </Button>
                            </div>
                          </div>
                        ))
                        : (
                          <div className='rounded-lg border border-dashed border-white/25 p-4 text-center text-sm text-default-400 dark:border-white/10'>
                            暂无模型
                          </div>
                          )}
                    </div>
                  </div>
                </div>

                <div className='space-y-3'>
                  <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                    <div className='mb-2 flex items-center justify-between'>
                      <div className='text-sm font-semibold text-default-700 dark:text-default-100'>请求体预览</div>
                      <div className='flex items-center gap-2'>
                        <Select
                          size='sm'
                          variant='bordered'
                          className='w-56'
                          selectedKeys={previewModelId ? [previewModelId] : []}
                          onSelectionChange={(keys) => {
                            const selected = Array.from(keys)[0];
                            if (typeof selected === 'string') {
                              setPreviewModelId(selected);
                            }
                          }}
                          placeholder='选择模型'
                        >
                          {(selectedProvider?.models ?? []).map((model) => (
                            <SelectItem key={model.id}>{model.id}</SelectItem>
                          ))}
                        </Select>
                        <Button
                          size='sm'
                          color='primary'
                          variant='flat'
                          isLoading={previewing}
                          onPress={() => void handlePreview()}
                        >
                          预览
                        </Button>
                      </div>
                    </div>

                    {previewData
                      ? (
                        <div className='space-y-2'>
                          <div className='rounded-lg border border-white/20 bg-white/55 px-3 py-2 text-xs text-default-500 dark:border-white/10 dark:bg-black/20'>
                            <div>model: {previewData.modelId}</div>
                            <div>method: {previewData.method}</div>
                            <div className='break-all'>endpoint: {previewData.endpoint}</div>
                          </div>
                          <Textarea
                            value={previewData.bodyText}
                            minRows={16}
                            variant='bordered'
                            readOnly
                          />
                          <Button
                            size='sm'
                            variant='flat'
                            startContent={<LuCopy />}
                            onPress={async () => {
                              try {
                                await navigator.clipboard.writeText(previewData.bodyText);
                                toast.success('预览内容已复制');
                              } catch (error) {
                                toast.error(`复制失败: ${(error as Error).message}`);
                              }
                            }}
                          >
                            复制预览
                          </Button>
                        </div>
                        )
                      : (
                        <div className='rounded-lg border border-dashed border-white/25 p-4 text-center text-sm text-default-400 dark:border-white/10'>
                          尚未生成预览
                        </div>
                        )}
                  </div>

                  <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                    <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>测试反馈</div>
                    <div className='flex items-center gap-2 text-sm text-default-500'>
                      <LuCircleCheck className='text-success' />
                      <span>成功/失败与 latency 会通过 toast 提示</span>
                    </div>
                    <div className='mt-2 flex items-center gap-2 text-sm text-default-500'>
                      <LuCircleX className='text-danger' />
                      <span>批量测试会逐条输出结果</span>
                    </div>
                  </div>
                </div>
              </div>
            </section>
          </div>
        </div>
      </div>
    </>
  );
}
