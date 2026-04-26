import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import {
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from '@heroui/modal';
import { Spinner } from '@heroui/spinner';
import { Tab, Tabs } from '@heroui/tabs';
import { useEffect, useMemo, useState } from 'react';
import toast from 'react-hot-toast';
import { IoMdOpen } from 'react-icons/io';

import PluginManager, {
  type ExtensionPageItem,
  type PluginCapabilitiesResponse,
  type PluginCapabilityListResponse,
  type PluginCapabilitySupportState,
  type PluginDiagnosticsResponse,
  type PluginExecutionRecord,
  type PluginItem,
  type PluginRuntimeStateResponse,
} from '@/controllers/plugin_manager';

interface PluginRuntimeDetailModalProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  plugin: PluginItem | null;
  extensionPages: ExtensionPageItem[];
  onConfig?: (plugin: PluginItem) => void;
}

function statusColor (status?: string): 'default' | 'success' | 'warning' | 'danger' | 'primary' {
  switch (status) {
    case 'active':
      return 'success';
    case 'registered_only':
    case 'deferred':
      return 'warning';
    case 'disabled':
      return 'default';
    case 'error':
      return 'danger';
    case 'unsupported':
      return 'default';
    default:
      return 'primary';
  }
}

function JsonPreview ({ value }: { value: unknown; }) {
  return (
    <pre className='max-h-56 overflow-auto rounded-lg border border-white/20 bg-white/50 p-3 text-xs text-default-600 dark:border-white/10 dark:bg-black/20 dark:text-default-300'>
      {JSON.stringify(value ?? {}, null, 2)}
    </pre>
  );
}

function SupportChip ({
  label,
  support,
}: {
  label: string;
  support?: PluginCapabilitySupportState;
}) {
  return (
    <Chip size='sm' variant='flat' color={statusColor(support?.status)}>
      {label}: {support?.status ?? 'unknown'}
    </Chip>
  );
}

function RuntimeRecord ({
  label,
  record,
}: {
  label: string;
  record?: PluginExecutionRecord;
}) {
  return (
    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
      <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>{label}</div>
      <div className='space-y-1 text-xs text-default-500'>
        <div>lastSuccessAt: {record?.lastSuccessAt || '-'}</div>
        <div>lastErrorAt: {record?.lastErrorAt || '-'}</div>
        <div className='break-words'>lastError: {record?.lastError || '-'}</div>
      </div>
    </div>
  );
}

function CapabilityItems ({
  title,
  data,
}: {
  title: string;
  data?: PluginCapabilityListResponse;
}) {
  const items = data?.items ?? [];

  return (
    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
      <div className='mb-3 flex flex-wrap items-center justify-between gap-2'>
        <div className='text-sm font-semibold text-default-700 dark:text-default-100'>{title}</div>
        <SupportChip label='status' support={data?.support} />
      </div>
      {items.length
        ? (
          <div className='space-y-2'>
            {items.map((item, index) => (
              <JsonPreview key={`${title}-${index}`} value={item} />
            ))}
          </div>
          )
        : (
          <div className='rounded-lg border border-dashed border-white/25 p-4 text-center text-sm text-default-400 dark:border-white/10'>
            暂无数据
          </div>
          )}
    </div>
  );
}

function pageUrl (pluginId: string, path: string) {
  return `/plugin/${pluginId}/page/${path.replace(/^\//, '')}`;
}

export default function PluginRuntimeDetailModal ({
  isOpen,
  onOpenChange,
  plugin,
  extensionPages,
  onConfig,
}: PluginRuntimeDetailModalProps) {
  const [loading, setLoading] = useState(false);
  const [capabilities, setCapabilities] = useState<PluginCapabilitiesResponse | null>(null);
  const [runtimeState, setRuntimeState] = useState<PluginRuntimeStateResponse | null>(null);
  const [diagnostics, setDiagnostics] = useState<PluginDiagnosticsResponse | null>(null);
  const [tools, setTools] = useState<PluginCapabilityListResponse | null>(null);
  const [webApis, setWebApis] = useState<PluginCapabilityListResponse | null>(null);
  const [cronJobs, setCronJobs] = useState<PluginCapabilityListResponse | null>(null);
  const [tasks, setTasks] = useState<PluginCapabilityListResponse | null>(null);

  const pluginPages = useMemo(() => {
    if (!plugin) {
      return [];
    }
    return extensionPages.filter((page) => page.pluginId === plugin.id);
  }, [extensionPages, plugin]);

  useEffect(() => {
    if (!isOpen || !plugin) {
      return;
    }

    const load = async () => {
      setLoading(true);
      setCapabilities(null);
      setRuntimeState(null);
      setDiagnostics(null);
      setTools(null);
      setWebApis(null);
      setCronJobs(null);
      setTasks(null);
      try {
        const [
          nextCapabilities,
          nextRuntimeState,
          nextDiagnostics,
          nextTools,
          nextWebApis,
          nextCronJobs,
          nextTasks,
        ] = await Promise.all([
          PluginManager.getPluginCapabilities(plugin.id),
          PluginManager.getPluginRuntimeState(plugin.id),
          PluginManager.getPluginDiagnostics(plugin.id),
          PluginManager.getPluginTools(plugin.id),
          PluginManager.getPluginWebApis(plugin.id),
          PluginManager.getPluginCronJobs(plugin.id),
          PluginManager.getPluginTasks(plugin.id),
        ]);
        setCapabilities(nextCapabilities);
        setRuntimeState(nextRuntimeState);
        setDiagnostics(nextDiagnostics);
        setTools(nextTools);
        setWebApis(nextWebApis);
        setCronJobs(nextCronJobs);
        setTasks(nextTasks);
      } catch (error) {
        toast.error(`加载插件运行时详情失败: ${(error as Error).message}`);
      } finally {
        setLoading(false);
      }
    };

    void load();
  }, [isOpen, plugin]);

  if (!plugin) {
    return null;
  }

  return (
    <Modal
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      size='5xl'
      scrollBehavior='inside'
      classNames={{
        backdrop: 'z-[200]',
        wrapper: 'z-[200]',
      }}
    >
      <ModalContent>
        {(onClose) => (
          <>
            <ModalHeader className='flex flex-col gap-2'>
              <div className='flex flex-wrap items-center gap-2'>
                <span className='text-xl font-bold'>{plugin.name}</span>
                <Chip size='sm' color={plugin.status === 'active' ? 'success' : 'default'} variant='flat'>
                  {plugin.status}
                </Chip>
                {plugin.runtimeKind && (
                  <Chip size='sm' variant='flat'>
                    {plugin.runtimeKind}
                  </Chip>
                )}
                {plugin.sourceKind && (
                  <Chip size='sm' color={plugin.sourceKind === 'astrbot-compatible' ? 'secondary' : 'default'} variant='flat'>
                    {plugin.sourceKind}
                  </Chip>
                )}
              </div>
              <div className='text-xs font-normal text-default-400'>
                {plugin.id}
              </div>
            </ModalHeader>
            <ModalBody className='relative min-h-[32rem]'>
              {loading && (
                <div className='absolute inset-0 z-10 flex items-center justify-center rounded-xl bg-white/40 backdrop-blur-sm dark:bg-black/20'>
                  <Spinner size='lg' />
                </div>
              )}
              <Tabs
                aria-label='Plugin Runtime Detail'
                classNames={{
                  tabList: 'bg-white/40 dark:bg-black/20 backdrop-blur-md',
                  cursor: 'bg-white/80 dark:bg-white/10 backdrop-blur-md shadow-sm',
                  panel: 'pt-4',
                }}
              >
                <Tab key='overview' title='Overview'>
                  <div className='grid gap-3 md:grid-cols-2'>
                    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
                      <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>基础信息</div>
                      <div className='space-y-1 text-sm text-default-500'>
                        <div>version: {plugin.version || '-'}</div>
                        <div>author: {plugin.author || '-'}</div>
                        <div>pluginType: {plugin.pluginType || '-'}</div>
                        <div>compatKind: {plugin.compatKind || '-'}</div>
                      </div>
                    </div>
                    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
                      <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>能力状态</div>
                      <div className='flex flex-wrap gap-2'>
                        <SupportChip label='tools' support={capabilities?.support.tools} />
                        <SupportChip label='webApis' support={capabilities?.support.webApis} />
                        <SupportChip label='cronJobs' support={capabilities?.support.cronJobs} />
                        <SupportChip label='tasks' support={capabilities?.support.tasks} />
                      </div>
                    </div>
                    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5 md:col-span-2'>
                      <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>描述</div>
                      <div className='text-sm leading-6 text-default-500'>
                        {plugin.description || '暂无描述'}
                      </div>
                    </div>
                  </div>
                </Tab>
                <Tab key='pages' title='Pages'>
                  {pluginPages.length
                    ? (
                      <div className='grid grid-cols-1 gap-3 md:grid-cols-2'>
                        {pluginPages.map((page) => (
                          <div key={`${page.pluginId}:${page.path}`} className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
                            <div className='mb-1 text-sm font-semibold text-default-700 dark:text-default-100'>
                              {page.title}
                            </div>
                            <div className='mb-3 text-xs text-default-400'>
                              {page.path}
                            </div>
                            {page.description && (
                              <div className='mb-3 text-sm text-default-500'>
                                {page.description}
                              </div>
                            )}
                            <Button
                              size='sm'
                              variant='flat'
                              startContent={<IoMdOpen />}
                              onPress={() => window.open(pageUrl(page.pluginId, page.path), '_blank')}
                            >
                              打开
                            </Button>
                          </div>
                        ))}
                      </div>
                      )
                    : (
                      <div className='rounded-xl border border-dashed border-white/25 p-8 text-center text-sm text-default-400 dark:border-white/10'>
                        暂无扩展页面
                      </div>
                      )}
                </Tab>
                <Tab key='capabilities' title='Capabilities'>
                  <div className='grid grid-cols-1 gap-3 xl:grid-cols-2'>
                    <CapabilityItems title='Tools' data={tools ?? undefined} />
                    <CapabilityItems title='Web APIs' data={webApis ?? undefined} />
                    <CapabilityItems title='Cron Jobs' data={cronJobs ?? undefined} />
                    <CapabilityItems title='Tasks' data={tasks ?? undefined} />
                  </div>
                </Tab>
                <Tab key='diagnostics' title='Diagnostics'>
                  <div className='grid gap-3 md:grid-cols-2'>
                    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
                      <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>Runtime State</div>
                      <JsonPreview value={runtimeState} />
                    </div>
                    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
                      <div className='mb-2 text-sm font-semibold text-default-700 dark:text-default-100'>Diagnostics</div>
                      <JsonPreview value={diagnostics} />
                    </div>
                    <RuntimeRecord label='Web API Dispatch' record={diagnostics?.lastWebApiDispatch} />
                    <RuntimeRecord label='Tool Execution' record={diagnostics?.lastToolExecution} />
                    <RuntimeRecord label='Cron Execution' record={diagnostics?.lastCronExecution} />
                  </div>
                </Tab>
              </Tabs>
            </ModalBody>
            <ModalFooter>
              {plugin.hasConfig && onConfig && (
                <Button variant='flat' onPress={() => onConfig(plugin)}>
                  配置
                </Button>
              )}
              <Button color='primary' onPress={onClose}>
                关闭
              </Button>
            </ModalFooter>
          </>
        )}
      </ModalContent>
    </Modal>
  );
}
