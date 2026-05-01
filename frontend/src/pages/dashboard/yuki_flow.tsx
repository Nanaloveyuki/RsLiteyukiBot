import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import { Input } from '@heroui/input';
import { Select, SelectItem } from '@heroui/select';
import { useRequest } from 'ahooks';
import { useEffect, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import toast from 'react-hot-toast';
import { LuBot, LuPlug, LuRefreshCw, LuShield, LuWrench } from 'react-icons/lu';

import SaveButtons from '@/components/button/save_buttons';
import PageLoading from '@/components/page_loading';
import SwitchCard from '@/components/switch_card';

import FlowLocalAgentManager from '@/controllers/flow_local_agent';

type FlowLocalAgentFormValues = {
  enabled: boolean;
  baseUrl: string;
  token: string;
  deviceId: string;
  deviceName: string;
  autoConnect: boolean;
  workspaceRoot: string;
  commandTimeoutSeconds: number;
  approvalPolicy: string;
  allowedTools: string[];
};

const TOOL_OPTIONS = [
  { key: 'read_file', label: 'read_file', description: '读取本地文件内容。' },
  { key: 'list_files', label: 'list_files', description: '列出目录内容。' },
  { key: 'write_file', label: 'write_file', description: '写入本地文件。' },
  { key: 'run_command', label: 'run_command', description: '执行本地命令。' },
] as const;

const APPROVAL_OPTIONS = [{ key: 'prompt', label: 'prompt' }] as const;

function statusChipColor (status: FlowLocalAgentStatus | undefined): 'success' | 'warning' | 'default' {
  if (status?.connected) {
    return 'success';
  }
  if (status?.reconnectAllowed) {
    return 'warning';
  }
  return 'default';
}

function statusLabel (status: FlowLocalAgentStatus | undefined) {
  if (status?.connected) {
    return '已连接';
  }
  if (status?.reconnectAllowed) {
    return '重连中';
  }
  return '未连接';
}

export default function YukiFlowPage () {
  const {
    data: configData,
    loading: configLoading,
    refreshAsync: refreshConfig,
  } = useRequest(FlowLocalAgentManager.getConfig);
  const {
    data: statusData,
    loading: statusLoading,
    refreshAsync: refreshStatus,
  } = useRequest(FlowLocalAgentManager.getStatus);
  const [saveEpoch, setSaveEpoch] = useState(0);

  const {
    control,
    handleSubmit,
    formState: { isSubmitting },
    setValue,
    watch,
  } = useForm<FlowLocalAgentFormValues>({
    defaultValues: {
      enabled: false,
      baseUrl: '',
      token: '',
      deviceId: '',
      deviceName: '',
      autoConnect: true,
      workspaceRoot: '',
      commandTimeoutSeconds: 30,
      approvalPolicy: 'prompt',
      allowedTools: ['read_file', 'list_files'],
    },
  });

  const enabled = watch('enabled');
  const allowedTools = watch('allowedTools');
  const toolSummary = allowedTools.length > 0
    ? String(allowedTools.length) + ' 个工具已启用'
    : '未启用任何工具';

  const reset = () => {
    if (!configData) {
      return;
    }

    setValue('enabled', configData.enabled);
    setValue('baseUrl', configData.baseUrl);
    setValue('token', configData.token);
    setValue('deviceId', configData.deviceId);
    setValue('deviceName', configData.deviceName);
    setValue('autoConnect', configData.autoConnect);
    setValue('workspaceRoot', configData.workspaceRoot);
    setValue('commandTimeoutSeconds', configData.commandTimeoutSeconds);
    setValue('approvalPolicy', configData.approvalPolicy || 'prompt');
    setValue(
      'allowedTools',
      configData.allowedTools.length > 0 ? configData.allowedTools : ['read_file', 'list_files']
    );
  };

  useEffect(() => {
    reset();
  }, [configData]);

  useEffect(() => {
    if (!saveEpoch) {
      return;
    }

    const timer = window.setTimeout(() => {
      void refreshStatus().catch(() => undefined);
    }, 1200);

    return () => window.clearTimeout(timer);
  }, [refreshStatus, saveEpoch]);

  const refreshAll = async (showToast = true) => {
    try {
      await Promise.all([refreshConfig(), refreshStatus()]);
      if (showToast) {
        toast.success('刷新成功');
      }
    } catch (error) {
      if (showToast) {
        toast.error('刷新失败: ' + (error as Error).message);
      }
      throw error;
    }
  };

  const onSubmit = handleSubmit(async (data) => {
    try {
      const next = await FlowLocalAgentManager.setConfig({
        ...data,
        baseUrl: data.baseUrl.trim(),
        token: data.token.trim(),
        deviceId: data.deviceId.trim(),
        deviceName: data.deviceName.trim(),
        workspaceRoot: data.workspaceRoot.trim(),
        approvalPolicy: data.approvalPolicy.trim() || 'prompt',
        commandTimeoutSeconds: Math.max(1, Number(data.commandTimeoutSeconds) || 30),
        allowedTools: data.allowedTools,
      });

      toast.success('保存成功');
      setSaveEpoch(Date.now());
      await Promise.all([refreshConfig(), refreshStatus()]);

      setValue('enabled', next.enabled);
      setValue('baseUrl', next.baseUrl);
      setValue('token', next.token);
      setValue('deviceId', next.deviceId);
      setValue('deviceName', next.deviceName);
      setValue('autoConnect', next.autoConnect);
      setValue('workspaceRoot', next.workspaceRoot);
      setValue('commandTimeoutSeconds', next.commandTimeoutSeconds);
      setValue('approvalPolicy', next.approvalPolicy);
      setValue('allowedTools', next.allowedTools);
    } catch (error) {
      toast.error('保存失败: ' + (error as Error).message);
    }
  });

  if (configLoading || statusLoading) {
    return <PageLoading loading />;
  }

  return (
    <>
      <title>Yuki Flow - Liteyuki WebUI</title>
      <div className='p-2 md:p-4'>
        <div className='mb-4 flex items-center justify-between gap-3'>
          <div className='flex items-center gap-2 text-default-700 dark:text-default-100'>
            <LuBot size={24} />
            <div>
              <h1 className='text-2xl font-bold'>Yuki Flow</h1>
              <p className='text-sm text-default-500'>管理 low_local_agent 的连接参数、只读工具范围与运行状态。</p>
            </div>
          </div>
          <Button size='sm' variant='flat' startContent={<LuRefreshCw />} onPress={() => void refreshAll()}>
            刷新
          </Button>
        </div>

        <div className='grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(20rem,0.8fr)]'>
          <div className='rounded-2xl border border-white/20 bg-white/55 p-4 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
            <div className='mb-3 flex items-center gap-2 text-default-700 dark:text-default-100'>
              <LuPlug />
              <span className='text-sm font-semibold'>连接配置</span>
            </div>

            <div className='grid gap-3'>
              <Controller
                control={control}
                name='enabled'
                render={({ field }) => (
                  <SwitchCard
                    value={field.value}
                    onValueChange={field.onChange}
                    label='启用 Yuki Flow'
                    description='启用后会按当前配置启动本地 Flow Agent 客户端。'
                  />
                )}
              />

              <Controller
                control={control}
                name='autoConnect'
                render={({ field }) => (
                  <SwitchCard
                    value={field.value}
                    onValueChange={field.onChange}
                    label='自动连接'
                    description='关闭后保留配置，但运行时不会主动建立到 Flow 的 WebSocket 连接。'
                  />
                )}
              />

              <Controller
                control={control}
                name='baseUrl'
                render={({ field }) => (
                  <Input
                    {...field}
                    label='Flow Base URL'
                    placeholder='https://flow.liteyuki.org'
                    description='将自动转换为 /ws/local-agent WebSocket 连接地址。'
                    isDisabled={!enabled}
                    classNames={{
                      inputWrapper:
                        'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                      input: 'bg-transparent text-default-700 placeholder:text-default-400',
                    }}
                  />
                )}
              />

              <Controller
                control={control}
                name='token'
                render={({ field }) => (
                  <Input
                    {...field}
                    type='password'
                    label='Agent Token'
                    placeholder='lys_xxx'
                    description='保存到本地配置，仅用于向 Flow 服务端鉴权。'
                    isDisabled={!enabled}
                    classNames={{
                      inputWrapper:
                        'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                      input: 'bg-transparent text-default-700 placeholder:text-default-400',
                    }}
                  />
                )}
              />

              <div className='grid gap-3 md:grid-cols-2'>
                <Controller
                  control={control}
                  name='deviceName'
                  render={({ field }) => (
                    <Input
                      {...field}
                      label='设备名称'
                      placeholder='My Server'
                      description='展示给 Flow 的设备名称。留空时将回退到主机名。'
                      classNames={{
                        inputWrapper:
                          'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                        input: 'bg-transparent text-default-700 placeholder:text-default-400',
                      }}
                    />
                  )}
                />

                <Controller
                  control={control}
                  name='deviceId'
                  render={({ field }) => (
                    <Input
                      {...field}
                      label='配置覆写 Device ID'
                      placeholder='留空则使用本地持久化生成值'
                      description='仅在需要固定设备身份时填写；留空时沿用本机持久化策略。'
                      classNames={{
                        inputWrapper:
                          'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                        input: 'bg-transparent text-default-700 placeholder:text-default-400',
                      }}
                    />
                  )}
                />
              </div>

              <Controller
                control={control}
                name='workspaceRoot'
                render={({ field }) => (
                  <Input
                    {...field}
                    label='Workspace Root'
                    placeholder='./'
                    description='供 Flow Agent 的本地工具作为默认工作区范围使用。'
                    classNames={{
                      inputWrapper:
                        'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                      input: 'bg-transparent text-default-700 placeholder:text-default-400',
                    }}
                  />
                )}
              />

              <div className='grid gap-3 md:grid-cols-2'>
                <Controller
                  control={control}
                  name='commandTimeoutSeconds'
                  render={({ field }) => (
                    <Input
                      {...field}
                      type='number'
                      label='命令超时（秒）'
                      placeholder='30'
                      value={field.value?.toString() ?? ''}
                      onChange={(event) => field.onChange(parseInt(event.target.value, 10) || 0)}
                      description='对 un_command 等执行型工具的超时上限。'
                      classNames={{
                        inputWrapper:
                          'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                        input: 'bg-transparent text-default-700 placeholder:text-default-400',
                      }}
                    />
                  )}
                />

                <Controller
                  control={control}
                  name='approvalPolicy'
                  render={({ field }) => (
                    <Select
                      label='审批策略'
                      selectedKeys={[field.value || 'prompt']}
                      onSelectionChange={(keys) => {
                        const value = Array.from(keys)[0]?.toString() || 'prompt';
                        field.onChange(value);
                      }}
                      description='当前阶段主要用于显式表达策略，后续沙盒模式会复用。'
                      classNames={{
                        trigger:
                          'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
                      }}
                    >
                      {APPROVAL_OPTIONS.map((option) => (
                        <SelectItem key={option.key} textValue={option.label}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </Select>
                  )}
                />
              </div>
            </div>

            <SaveButtons
              onSubmit={onSubmit}
              reset={reset}
              isSubmitting={isSubmitting}
              refresh={() => void refreshAll()}
            />
          </div>

          <div className='space-y-4'>
            <div className='rounded-2xl border border-white/20 bg-white/55 p-4 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
              <div className='mb-3 flex items-center gap-2 text-default-700 dark:text-default-100'>
                <LuShield />
                <span className='text-sm font-semibold'>运行状态</span>
              </div>
              <div className='grid gap-3'>
                <div className='flex items-center justify-between rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div>
                    <div className='text-sm font-semibold text-default-700 dark:text-default-100'>连接状态</div>
                    <div className='text-xs text-default-500'>Flow Agent 当前与上游 Flow 服务的连接快照。</div>
                  </div>
                  <Chip color={statusChipColor(statusData)} variant='flat'>
                    {statusLabel(statusData)}
                  </Chip>
                </div>

                <div className='grid gap-3 sm:grid-cols-2'>
                  <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                    <div className='text-xs text-default-400'>自动重连</div>
                    <div className='mt-1 text-sm font-semibold text-default-700 dark:text-default-100'>
                      {statusData?.reconnectAllowed ? '允许' : '未启用'}
                    </div>
                  </div>
                  <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                    <div className='text-xs text-default-400'>当前生效 Device ID</div>
                    <div className='mt-1 break-all text-sm font-semibold text-default-700 dark:text-default-100'>
                      {configData?.effectiveDeviceId || '未生成'}
                    </div>
                  </div>
                </div>

                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='text-xs text-default-400'>最近错误</div>
                  <div className='mt-1 break-words text-sm text-default-600 dark:text-default-300'>
                    {statusData?.lastError || '暂无'}
                  </div>
                </div>

                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='text-xs text-default-400'>配置文件</div>
                  <div className='mt-1 break-all text-sm text-default-600 dark:text-default-300'>
                    {configData?.configPath || '未知'}
                  </div>
                </div>
              </div>
            </div>

            <div className='rounded-2xl border border-white/20 bg-white/55 p-4 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
              <div className='mb-3 flex items-center gap-2 text-default-700 dark:text-default-100'>
                <LuWrench />
                <span className='text-sm font-semibold'>工具范围</span>
              </div>
              <div className='mb-3 text-sm text-default-500'>
                当前共享层优先保障 workspace 内的只读能力；主链路暂不直接接入，后续会在沙盒模式下复用同一边界。
              </div>
              <div className='mb-3 flex items-center justify-between rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                <div>
                  <div className='text-sm font-semibold text-default-700 dark:text-default-100'>已启用工具</div>
                  <div className='text-xs text-default-500'>{toolSummary}</div>
                </div>
                <Chip variant='flat' color='primary'>
                  {allowedTools.length}
                </Chip>
              </div>
              <div className='grid gap-3'>
                {TOOL_OPTIONS.map((tool) => {
                  const selected = allowedTools.includes(tool.key);
                  return (
                    <SwitchCard
                      key={tool.key}
                      value={selected}
                      onValueChange={(value) => {
                        const next = value
                          ? Array.from(new Set([...allowedTools, tool.key]))
                          : allowedTools.filter((item) => item !== tool.key);
                        setValue('allowedTools', next, { shouldDirty: true });
                      }}
                      label={tool.label}
                      description={tool.description}
                    />
                  );
                })}
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
