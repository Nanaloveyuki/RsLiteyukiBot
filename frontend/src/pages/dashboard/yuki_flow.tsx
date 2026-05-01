import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import { Input, Textarea } from '@heroui/input';
import { Select, SelectItem } from '@heroui/select';
import type { Selection } from '@react-types/shared';
import { useRequest } from 'ahooks';
import { useEffect, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import toast from 'react-hot-toast';
import {
  LuActivity,
  LuBot,
  LuCopy,
  LuFileText,
  LuInfo,
  LuKeyRound,
  LuLink,
  LuPlug,
  LuRefreshCw,
  LuShield,
  LuUnplug,
  LuWrench,
} from 'react-icons/lu';

import LogLevelSelect from '@/components/log_com/log_level_select';
import SaveButtons from '@/components/button/save_buttons';
import PageLoading from '@/components/page_loading';
import SwitchCard from '@/components/switch_card';
import { LogLevel } from '@/const/enum';
import FlowLocalAgentManager from '@/controllers/flow_local_agent';

type FlowLocalAgentFormValues = {
  enabled: boolean;
  baseUrl: string;
  deviceId: string;
  deviceName: string;
  autoConnect: boolean;
  workspaceRoot: string;
  commandTimeoutSeconds: number;
  approvalPolicy: string;
  allowedTools: string[];
};

type ManualTokenFormValues = {
  token: string;
};

const TOOL_OPTIONS = [
  { key: 'read_file', label: 'read_file', description: '读取本地文件内容。' },
  { key: 'list_files', label: 'list_files', description: '列出目录内容。' },
  { key: 'write_file', label: 'write_file', description: '写入本地文件。' },
  { key: 'run_command', label: 'run_command', description: '执行本地命令。' },
] as const;

const APPROVAL_OPTIONS = [{ key: 'prompt', label: 'prompt' }] as const;

const INPUT_CLASSNAMES = {
  inputWrapper:
    'bg-default-100/50 dark:bg-white/5 backdrop-blur-md border border-transparent hover:bg-default-200/50 dark:hover:bg-white/10 transition-all shadow-sm data-[hover=true]:border-default-300',
  input: 'bg-transparent text-default-700 placeholder:text-default-400',
};

function statusChipColor (status: FlowLocalAgentStatus | undefined): 'success' | 'warning' | 'default' {
  if (status?.connected) {
    return 'success';
  }
  if (status?.reconnectAllowed) {
    return 'warning';
  }
  return 'default';
}

function logLevelChipColor (level: LogLevel): 'default' | 'primary' | 'warning' | 'danger' {
  switch (level) {
    case LogLevel.DEBUG:
      return 'default';
    case LogLevel.INFO:
      return 'primary';
    case LogLevel.WARN:
      return 'warning';
    case LogLevel.ERROR:
    case LogLevel.FATAL:
      return 'danger';
    default:
      return 'default';
  }
}

function normalizeLogLevel (level: string): LogLevel {
  switch (level.trim().toLowerCase()) {
    case LogLevel.DEBUG:
      return LogLevel.DEBUG;
    case LogLevel.WARN:
      return LogLevel.WARN;
    case LogLevel.ERROR:
      return LogLevel.ERROR;
    case LogLevel.FATAL:
      return LogLevel.FATAL;
    case LogLevel.INFO:
    default:
      return LogLevel.INFO;
  }
}

function isLogLevelSelected (selection: Selection, level: LogLevel) {
  if (selection === 'all') {
    return true;
  }
  return selection.has(level);
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

function openExternalLink (url: string) {
  window.open(url, '_blank', 'noopener,noreferrer');
}

async function copyToClipboard (value: string, label: string) {
  await navigator.clipboard.writeText(value);
  toast.success(`${label}已复制`);
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
  const {
    data: logData,
    loading: logsLoading,
    refreshAsync: refreshLogs,
  } = useRequest(FlowLocalAgentManager.getLogs, {
    pollingInterval: 3000,
  });
  const [saveEpoch, setSaveEpoch] = useState(0);
  const [deviceCodeState, setDeviceCodeState] = useState<FlowLocalAgentDeviceCode | null>(null);
  const [deviceCodeStatus, setDeviceCodeStatus] = useState<string>('');
  const [deviceCodeBusy, setDeviceCodeBusy] = useState(false);
  const [runtimeActionBusy, setRuntimeActionBusy] = useState(false);
  const [logLevels, setLogLevels] = useState<Selection>(
    new Set([LogLevel.DEBUG, LogLevel.INFO, LogLevel.WARN, LogLevel.ERROR])
  );

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
      deviceId: '',
      deviceName: '',
      autoConnect: true,
      workspaceRoot: '',
      commandTimeoutSeconds: 30,
      approvalPolicy: 'prompt',
      allowedTools: ['read_file', 'list_files'],
    },
  });

  const {
    control: tokenControl,
    handleSubmit: handleTokenSubmit,
    formState: { isSubmitting: isSubmittingToken },
    reset: resetTokenForm,
  } = useForm<ManualTokenFormValues>({
    defaultValues: {
      token: '',
    },
  });

  const enabled = watch('enabled');
  const allowedTools = watch('allowedTools');
  const baseUrl = watch('baseUrl');
  const toolSummary = allowedTools.length > 0
    ? `${String(allowedTools.length)} 个工具已启用`
    : '未启用任何工具';
  const flowLogs = (logData?.entries || [])
    .map((entry) => ({
      ...entry,
      normalizedLevel: normalizeLogLevel(entry.level),
    }))
    .filter((entry) => isLogLevelSelected(logLevels, entry.normalizedLevel))
    .slice()
    .reverse();

  const reset = () => {
    if (!configData) {
      return;
    }

    setValue('enabled', configData.enabled);
    setValue('baseUrl', configData.baseUrl);
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
      await Promise.all([refreshConfig(), refreshStatus(), refreshLogs()]);
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
        deviceId: data.deviceId.trim(),
        deviceName: data.deviceName.trim(),
        workspaceRoot: data.workspaceRoot.trim(),
        approvalPolicy: data.approvalPolicy.trim() || 'prompt',
        commandTimeoutSeconds: Math.max(1, Number(data.commandTimeoutSeconds) || 30),
        allowedTools: data.allowedTools,
      });

      toast.success('保存成功，已按最新配置重启连接');
      setSaveEpoch(Date.now());
      await Promise.all([refreshConfig(), refreshStatus()]);

      setValue('enabled', next.enabled);
      setValue('baseUrl', next.baseUrl);
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

  const onConnectNow = async () => {
    try {
      setRuntimeActionBusy(true);
      await FlowLocalAgentManager.connectNow();
      setSaveEpoch(Date.now());
      await Promise.all([refreshConfig(), refreshStatus(), refreshLogs()]);
      toast.success('已触发立即连接');
    } catch (error) {
      toast.error('立即连接失败: ' + (error as Error).message);
    } finally {
      setRuntimeActionBusy(false);
    }
  };

  const onDisconnectNow = async () => {
    try {
      setRuntimeActionBusy(true);
      await FlowLocalAgentManager.disconnectNow();
      setSaveEpoch(Date.now());
      await Promise.all([refreshStatus(), refreshLogs()]);
      toast.success('已断开当前连接');
    } catch (error) {
      toast.error('断开连接失败: ' + (error as Error).message);
    } finally {
      setRuntimeActionBusy(false);
    }
  };

  const onSaveToken = handleTokenSubmit(async (data) => {
    try {
      await FlowLocalAgentManager.setToken({ token: data.token.trim() });
      resetTokenForm({ token: '' });
      await refreshAll(false);
      toast.success('Token 已更新');
    } catch (error) {
      toast.error('保存 Token 失败: ' + (error as Error).message);
    }
  });

  const onStartDeviceCodeLogin = async () => {
    try {
      setDeviceCodeBusy(true);
      const next = await FlowLocalAgentManager.startDeviceCode({
        baseUrl: baseUrl.trim() || undefined,
      });
      setDeviceCodeState(next);
      setDeviceCodeStatus('等待在 Flow 页面完成授权');
      toast.success('设备码已生成');
    } catch (error) {
      toast.error('启动设备码登录失败: ' + (error as Error).message);
    } finally {
      setDeviceCodeBusy(false);
    }
  };

  const onPollDeviceCode = async () => {
    if (!deviceCodeState) {
      return;
    }

    try {
      setDeviceCodeBusy(true);
      const result = await FlowLocalAgentManager.pollDeviceCode({
        baseUrl: baseUrl.trim() || undefined,
        deviceCode: deviceCodeState.deviceCode,
      });

      if (result.status === 'approved' && result.hasToken) {
        setDeviceCodeStatus('授权成功，Token 已保存到本地配置');
        await refreshAll(false);
        toast.success('设备登录成功');
        return;
      }
      if (result.status === 'expired') {
        setDeviceCodeStatus('当前设备码已过期，请重新发起登录');
        toast.error('设备码已过期');
        return;
      }

      setDeviceCodeStatus('授权尚未完成，请在 Flow 页面确认后重试');
      toast('等待授权完成');
    } catch (error) {
      toast.error('检查授权结果失败: ' + (error as Error).message);
    } finally {
      setDeviceCodeBusy(false);
    }
  };

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
              <p className='text-sm text-default-500'>
                管理 Yuki Flow 的连接参数、登录方式、可用工具与运行状态。
              </p>
            </div>
          </div>
          <Button size='sm' variant='flat' startContent={<LuRefreshCw />} onPress={() => void refreshAll()}>
            刷新
          </Button>
        </div>

        <div className='grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(20rem,0.8fr)]'>
          <div className='space-y-4'>
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
                      description='运行时会将该地址转换为 /ws/local-agent 的出站 WebSocket 连接。'
                      isDisabled={!enabled}
                      classNames={INPUT_CLASSNAMES}
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
                        description='展示给 Flow 的设备名称。留空时会回退到主机名。'
                        classNames={INPUT_CLASSNAMES}
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
                        description='仅在需要固定设备身份时填写。'
                        classNames={INPUT_CLASSNAMES}
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
                      classNames={INPUT_CLASSNAMES}
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
                        description='对 run_command 等执行型工具的超时上限。'
                        classNames={INPUT_CLASSNAMES}
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
                        description='执行需要确认的操作时，当前会使用提示确认。'
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

            <div className='rounded-2xl border border-white/20 bg-white/55 p-4 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
              <div className='mb-3 flex items-center gap-2 text-default-700 dark:text-default-100'>
                <LuKeyRound />
                <span className='text-sm font-semibold'>登录与凭据</span>
              </div>

              <div className='grid gap-3'>
                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='flex items-center justify-between gap-3'>
                    <div>
                      <div className='text-sm font-semibold text-default-700 dark:text-default-100'>当前 Token 状态</div>
                      <div className='text-xs text-default-500'>用于连接 Flow 服务的登录状态。</div>
                    </div>
                    <Chip color={configData?.hasToken ? 'success' : 'default'} variant='flat'>
                      {configData?.hasToken ? '已保存' : '未配置'}
                    </Chip>
                  </div>
                  <div className='mt-3 text-sm text-default-600 dark:text-default-300'>
                    {configData?.tokenPreview || '当前未保存 Token'}
                  </div>
                </div>

                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='mb-3'>
                    <div className='text-sm font-semibold text-default-700 dark:text-default-100'>设备码登录</div>
                    <div className='text-xs text-default-500'>
                      推荐方式。生成验证码后，在 Flow 页面完成授权即可。
                    </div>
                  </div>
                  <div className='flex flex-wrap gap-2'>
                    <Button
                      color='primary'
                      variant='flat'
                      isLoading={deviceCodeBusy}
                      onPress={() => void onStartDeviceCodeLogin()}
                    >
                      开始设备码登录
                    </Button>
                    <Button
                      variant='flat'
                      isDisabled={!deviceCodeState || deviceCodeBusy}
                      isLoading={deviceCodeBusy && !!deviceCodeState}
                      onPress={() => void onPollDeviceCode()}
                    >
                      检查授权结果
                    </Button>
                  </div>

                  {deviceCodeState && (
                    <div className='mt-3 grid gap-3'>
                      <Input
                        isReadOnly
                        label='验证码'
                        value={deviceCodeState.userCode}
                        description='在 Flow 页面输入该验证码完成授权。'
                        endContent={
                          <Button isIconOnly size='sm' variant='light' onPress={() => void copyToClipboard(deviceCodeState.userCode, '验证码')}>
                            <LuCopy />
                          </Button>
                        }
                        classNames={INPUT_CLASSNAMES}
                      />
                      <Textarea
                        isReadOnly
                        label='验证页面'
                        value={deviceCodeState.verificationUrl}
                        minRows={2}
                        description={`有效期 ${deviceCodeState.expiresIn} 秒`}
                        classNames={{
                          inputWrapper: INPUT_CLASSNAMES.inputWrapper,
                          input: INPUT_CLASSNAMES.input,
                        }}
                      />
                      <div className='flex flex-wrap gap-2'>
                        <Button
                          variant='flat'
                          startContent={<LuLink />}
                          onPress={() => openExternalLink(deviceCodeState.verificationUrl)}
                        >
                          打开验证页面
                        </Button>
                        <Button
                          variant='flat'
                          startContent={<LuCopy />}
                          onPress={() => void copyToClipboard(deviceCodeState.verificationUrl, '验证链接')}
                        >
                          复制验证链接
                        </Button>
                      </div>
                    </div>
                  )}

                  <div className='mt-3 text-sm text-default-600 dark:text-default-300'>
                    {deviceCodeStatus || '尚未发起设备码登录'}
                  </div>
                </div>

                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='mb-3'>
                    <div className='text-sm font-semibold text-default-700 dark:text-default-100'>手工写入 Token</div>
                    <div className='text-xs text-default-500'>
                      适用于已经有可用 Token 的情况。建议优先使用设备码登录。
                    </div>
                  </div>
                  <div className='grid gap-3'>
                    <Controller
                      control={tokenControl}
                      name='token'
                      render={({ field }) => (
                        <Input
                          {...field}
                          type='password'
                          label='Agent Token'
                          placeholder='lys_xxx'
                          description='提交后由后端持久化，不会再回显明文。'
                          classNames={INPUT_CLASSNAMES}
                        />
                      )}
                    />
                    <div className='flex justify-end'>
                      <Button
                        color='primary'
                        radius='full'
                        isLoading={isSubmittingToken}
                        onPress={() => void onSaveToken()}
                      >
                        更新 Token
                      </Button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
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

                <div className='flex justify-end'>
                  <div className='flex flex-wrap gap-2'>
                    <Button
                      color='primary'
                      variant='flat'
                      isLoading={runtimeActionBusy}
                      onPress={() => void onConnectNow()}
                    >
                      立即连接
                    </Button>
                    <Button
                      color='danger'
                      variant='flat'
                      isLoading={runtimeActionBusy}
                      onPress={() => void onDisconnectNow()}
                    >
                      断开连接
                    </Button>
                  </div>
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

                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='mb-2 flex items-center gap-2 text-default-700 dark:text-default-100'>
                    <LuInfo size={16} />
                    <span className='text-sm font-semibold'>连接机制说明</span>
                  </div>
                  <div className='space-y-2 text-sm text-default-600 dark:text-default-300'>
                    <div>保存配置后会立即应用，并尝试按最新设置建立连接。</div>
                    <div>连接失败时会自动重试，你也可以随时手动发起连接。</div>
                    <div>点击“断开连接”会停止当前连接，直到你再次手动连接或重新保存配置。</div>
                  </div>
                </div>
              </div>
            </div>

            <div className='rounded-2xl border border-white/20 bg-white/55 p-4 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
              <div className='mb-3 flex items-center gap-2 text-default-700 dark:text-default-100'>
                <LuWrench />
                <span className='text-sm font-semibold'>可用工具</span>
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

            <div className='rounded-2xl border border-white/20 bg-white/55 p-4 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
              <div className='mb-3 flex items-center gap-2 text-default-700 dark:text-default-100'>
                <LuFileText />
                <span className='text-sm font-semibold'>Flow 日志</span>
              </div>

              <div className='mb-3 flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between'>
                <div className='text-sm text-default-500'>
                  显示当前连接与工具执行相关的最近日志。需要更详细日志时，请将后端日志级别调整为 debug。
                </div>
                <div className='flex flex-wrap items-center gap-2'>
                  <div className='min-w-[14rem]'>
                    <LogLevelSelect selectedKeys={logLevels} onSelectionChange={setLogLevels} />
                  </div>
                  <Button
                    size='sm'
                    variant='flat'
                    startContent={<LuRefreshCw />}
                    onPress={() => void refreshLogs()}
                  >
                    刷新日志
                  </Button>
                </div>
              </div>

              <div className='mb-3 grid gap-3 sm:grid-cols-3'>
                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='text-xs text-default-400'>显示条目</div>
                  <div className='mt-1 text-sm font-semibold text-default-700 dark:text-default-100'>
                    {flowLogs.length}
                  </div>
                </div>
                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='text-xs text-default-400'>刷新方式</div>
                  <div className='mt-1 text-sm font-semibold text-default-700 dark:text-default-100'>
                    每 3 秒轮询
                  </div>
                </div>
                <div className='rounded-xl border border-white/20 bg-white/40 p-3 dark:border-white/10 dark:bg-white/5'>
                  <div className='text-xs text-default-400'>日志范围</div>
                  <div className='mt-1 text-sm font-semibold text-default-700 dark:text-default-100'>
                    当前连接会话
                  </div>
                </div>
              </div>

              <div className='max-h-[32rem] space-y-2 overflow-y-auto rounded-xl border border-white/20 bg-white/35 p-3 dark:border-white/10 dark:bg-white/5'>
                {logsLoading && flowLogs.length === 0 && <PageLoading loading />}
                {!logsLoading && flowLogs.length === 0 && (
                  <div className='flex items-center gap-2 text-sm text-default-500'>
                    <LuActivity size={16} />
                    <span>当前没有可显示的 Flow 日志。</span>
                  </div>
                )}
                {flowLogs.map((entry, index) => (
                  <div
                    key={`${entry.timestamp}-${entry.module}-${String(index)}`}
                    className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-black/20'
                  >
                    <div className='mb-2 flex flex-wrap items-center gap-2'>
                      <Chip size='sm' variant='flat' color={logLevelChipColor(entry.normalizedLevel)}>
                        {entry.normalizedLevel.toUpperCase()}
                      </Chip>
                      <span className='text-xs text-default-400'>{entry.timestamp}</span>
                    </div>
                    <div className='break-words text-sm text-default-700 dark:text-default-200'>
                      {entry.message}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
