import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import { Input, Textarea } from '@heroui/input';
import { Switch } from '@heroui/switch';
import { Tab, Tabs } from '@heroui/tabs';
import { type ChangeEvent, type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import toast from 'react-hot-toast';
import {
  LuBookOpen,
  LuBraces,
  LuChevronRight,
  LuFileArchive,
  LuFileText,
  LuFolderOpen,
  LuPlus,
  LuRefreshCw,
  LuSave,
  LuSearch,
  LuServer,
  LuTrash2,
  LuUpload,
  LuWrench,
} from 'react-icons/lu';

import PageLoading from '@/components/page_loading';
import CapabilityManager, {
  type McpServerItem,
  type SkillInventoryItem,
  type SkillReadResponse,
  type ToolInventoryItem,
} from '@/controllers/capability_manager';

const DISCOVERY_HELPERS = new Set([
  'list_tool_categories',
  'list_tools_in_category',
  'get_tool_schema',
]);

const EMPTY_SERVER: McpServerItem = {
  name: '',
  transport: 'streamable_http',
  url: '',
  active: true,
  headers: {},
};

function WarningList ({ warnings }: { warnings: string[]; }) {
  if (!warnings.length) {
    return null;
  }

  return (
    <div className='rounded-xl border border-warning/20 bg-warning/10 p-3 text-sm text-warning-700 dark:text-warning-300'>
      {warnings.map((warning) => (
        <div key={warning} className='break-words'>
          {warning}
        </div>
      ))}
    </div>
  );
}

function JsonPreview ({ value }: { value: unknown; }) {
  return (
    <details className='group rounded-lg border border-white/20 bg-white/50 dark:border-white/10 dark:bg-black/20'>
      <summary className='flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-xs font-medium text-default-600 dark:text-default-300'>
        <LuChevronRight className='transition-transform group-open:rotate-90' />
        <span>详细信息 JSON</span>
      </summary>
      <pre className='max-h-56 overflow-auto border-t border-white/20 p-3 text-xs text-default-600 dark:border-white/10 dark:text-default-300'>
        {JSON.stringify(value ?? {}, null, 2)}
      </pre>
    </details>
  );
}

function normalizeSearchText (value: string) {
  return value.trim().toLowerCase();
}

function matchesQuery (query: string, parts: Array<string | undefined>) {
  const normalized = normalizeSearchText(query);
  if (!normalized) {
    return true;
  }
  return parts.some((part) => part?.toLowerCase().includes(normalized));
}

function uniqueTags (values: Array<string | undefined>) {
  return Array.from(new Set(values
    .map((value) => value?.trim())
    .filter((value): value is string => !!value)))
    .sort((left, right) => left.localeCompare(right));
}

function FilterToolbar ({
  query,
  onQueryChange,
  tags,
  activeTag,
  onTagChange,
  placeholder,
  total,
  shown,
}: {
  query: string;
  onQueryChange: (value: string) => void;
  tags?: string[];
  activeTag?: string;
  onTagChange?: (value: string) => void;
  placeholder: string;
  total: number;
  shown: number;
}) {
  return (
    <div className='mt-3 rounded-xl border border-white/20 bg-white/35 p-3 dark:border-white/10 dark:bg-white/5'>
      <div className='flex flex-col gap-2 md:flex-row md:items-center'>
        <Input
          size='sm'
          variant='bordered'
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={placeholder}
          startContent={<LuSearch className='text-default-400' />}
        />
        <div className='shrink-0 text-xs text-default-400'>
          {shown}/{total}
        </div>
      </div>
      {tags && tags.length > 0 && onTagChange && (
        <div className='mt-2 flex flex-wrap gap-1.5'>
          <Chip
            size='sm'
            variant={!activeTag ? 'solid' : 'flat'}
            color={!activeTag ? 'primary' : 'default'}
            className='cursor-pointer'
            onClick={() => onTagChange('')}
          >
            全部
          </Chip>
          {tags.map((tag) => (
            <Chip
              key={tag}
              size='sm'
              variant={activeTag === tag ? 'solid' : 'flat'}
              color={activeTag === tag ? 'primary' : 'default'}
              className='cursor-pointer'
              onClick={() => onTagChange(tag)}
            >
              {tag}
            </Chip>
          ))}
        </div>
      )}
    </div>
  );
}

function SectionTitle ({
  icon,
  title,
  action,
}: {
  icon: ReactNode;
  title: string;
  action?: ReactNode;
}) {
  return (
    <div className='mb-3 flex items-center justify-between gap-3'>
      <div className='flex items-center gap-2 text-default-700 dark:text-default-100'>
        {icon}
        <span className='text-sm font-semibold'>{title}</span>
      </div>
      {action}
    </div>
  );
}

function ToolCard ({
  tool,
  onToggle,
}: {
  tool: ToolInventoryItem;
  onToggle: (name: string, active: boolean) => void;
}) {
  const locked = DISCOVERY_HELPERS.has(tool.name);

  return (
    <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
      <div className='flex items-start justify-between gap-3'>
        <div className='min-w-0'>
          <div className='truncate text-sm font-semibold text-default-800 dark:text-default-100'>
            {tool.name}
          </div>
          <div className='mt-1 text-xs text-default-500'>
            {tool.description || '暂无描述'}
          </div>
        </div>
        <Switch
          size='sm'
          isDisabled={locked}
          isSelected={locked || tool.active !== false}
          onValueChange={(active) => onToggle(tool.name, active)}
        />
      </div>
      <div className='mt-3 flex flex-wrap gap-2'>
        <Chip size='sm' variant='flat' color='primary'>
          {tool.category || 'uncategorized'}
        </Chip>
        <Chip size='sm' variant='flat' color={tool.origin?.startsWith('mcp:') ? 'secondary' : 'default'}>
          {tool.origin || 'local'}
        </Chip>
        {tool.strict && (
          <Chip size='sm' variant='flat' color='success'>
            strict
          </Chip>
        )}
        {locked && (
          <Chip size='sm' variant='flat' color='warning'>
            required
          </Chip>
        )}
      </div>
      {tool.whenToUse && (
        <div className='mt-3 text-xs leading-5 text-default-500'>
          {tool.whenToUse}
        </div>
      )}
      <div className='mt-3'>
        <JsonPreview value={tool.parameters} />
      </div>
    </div>
  );
}

function ToolsPanel () {
  const [loading, setLoading] = useState(true);
  const [tools, setTools] = useState<ToolInventoryItem[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [query, setQuery] = useState('');
  const [tag, setTag] = useState('');

  const load = async () => {
    setLoading(true);
    try {
      const data = await CapabilityManager.getTools();
      setTools(data.tools);
      setWarnings(data.warnings ?? []);
    } catch (error) {
      toast.error(`加载工具失败: ${(error as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const handleToggle = async (name: string, active: boolean) => {
    const loadingToast = toast.loading(active ? '启用工具中...' : '禁用工具中...');
    try {
      const data = await CapabilityManager.toggleTool(name, active);
      setTools(data.tools);
      setWarnings(data.warnings ?? []);
      toast.success(active ? '工具已启用' : '工具已禁用', { id: loadingToast });
    } catch (error) {
      toast.error((error as Error).message, { id: loadingToast });
    }
  };

  const tags = useMemo(
    () => uniqueTags(tools.flatMap((tool) => [tool.category, tool.origin])),
    [tools]
  );
  const filteredTools = useMemo(
    () => tools.filter((tool) => {
      const tagMatched = !tag || tool.category === tag || tool.origin === tag;
      return tagMatched && matchesQuery(query, [
        tool.name,
        tool.description,
        tool.category,
        tool.origin,
        tool.whenToUse,
      ]);
    }),
    [query, tag, tools]
  );

  return (
    <div className='relative'>
      <PageLoading loading={loading} />
      <SectionTitle
        icon={<LuWrench />}
        title='Tools'
        action={(
          <Button size='sm' variant='flat' startContent={<LuRefreshCw />} onPress={() => void load()}>
            刷新
          </Button>
        )}
      />
      <WarningList warnings={warnings} />
      <FilterToolbar
        query={query}
        onQueryChange={setQuery}
        tags={tags}
        activeTag={tag}
        onTagChange={setTag}
        placeholder='搜索工具名称、描述、类别或来源'
        total={tools.length}
        shown={filteredTools.length}
      />
      <div className='mt-3 grid grid-cols-1 gap-3 xl:grid-cols-2'>
        {filteredTools.map((tool) => (
          <ToolCard key={`${tool.origin ?? 'local'}:${tool.name}`} tool={tool} onToggle={handleToggle} />
        ))}
        {!filteredTools.length && !loading && (
          <div className='rounded-xl border border-dashed border-white/25 p-6 text-center text-sm text-default-400 dark:border-white/10'>
            {tools.length ? '没有匹配的工具' : '暂无工具'}
          </div>
        )}
      </div>
    </div>
  );
}

function normalizeServer (server: McpServerItem): McpServerItem {
  return {
    ...server,
    name: server.name.trim(),
    transport: server.transport.trim() || 'streamable_http',
    url: server.url.trim(),
    active: server.active,
    headers: server.headers ?? {},
  };
}

function McpPanel () {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [servers, setServers] = useState<McpServerItem[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [configPath, setConfigPath] = useState('');
  const [query, setQuery] = useState('');
  const [tag, setTag] = useState('');

  const load = async () => {
    setLoading(true);
    try {
      const data = await CapabilityManager.getMcpServers();
      setServers(data.servers);
      setWarnings(data.warnings ?? []);
      setConfigPath(data.configPath ?? '');
    } catch (error) {
      toast.error(`加载 MCP 失败: ${(error as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const updateServer = (index: number, updater: (server: McpServerItem) => McpServerItem) => {
    setServers((current) => current.map((server, currentIndex) => (
      currentIndex === index ? updater(server) : server
    )));
  };

  const save = async () => {
    const payload = servers.map(normalizeServer).filter((server) => server.name && server.url);
    if (!payload.length) {
      toast.error('至少保留一个有效 MCP Server');
      return;
    }
    setSaving(true);
    try {
      const data = await CapabilityManager.saveMcpServers(payload);
      setServers(data.servers);
      setWarnings(data.warnings ?? []);
      setConfigPath(data.configPath ?? '');
      toast.success('MCP 配置已保存');
    } catch (error) {
      toast.error(`保存 MCP 失败: ${(error as Error).message}`);
    } finally {
      setSaving(false);
    }
  };

  const test = async (server: McpServerItem) => {
    const candidate = normalizeServer(server);
    if (!candidate.name || !candidate.url) {
      toast.error('请先填写 name 和 url');
      return;
    }
    const loadingToast = toast.loading('测试 MCP Server...');
    try {
      const data = await CapabilityManager.testMcpServer(candidate);
      const mergedWarnings = [
        ...(data.warnings ?? []),
        ...(data.servers.flatMap((item) => item.warnings ?? [])),
      ];
      toast.success(mergedWarnings.length ? '测试完成，请查看 warning' : '测试通过', { id: loadingToast });
      setWarnings(mergedWarnings);
    } catch (error) {
      toast.error((error as Error).message, { id: loadingToast });
    }
  };

  const tags = useMemo(
    () => uniqueTags(servers.flatMap((server) => [
      server.active ? 'active' : 'disabled',
      server.transport,
    ])),
    [servers]
  );
  const filteredServers = useMemo(
    () => servers.filter((server) => {
      const statusTag = server.active ? 'active' : 'disabled';
      const tagMatched = !tag || server.transport === tag || statusTag === tag;
      return tagMatched && matchesQuery(query, [
        server.name,
        server.transport,
        server.url,
        ...(server.toolNames ?? []),
        ...(server.warnings ?? []),
      ]);
    }),
    [query, servers, tag]
  );

  return (
    <div className='relative'>
      <PageLoading loading={loading} />
      <SectionTitle
        icon={<LuServer />}
        title='MCP Servers'
        action={(
          <div className='flex items-center gap-2'>
            <Button size='sm' variant='flat' startContent={<LuPlus />} onPress={() => setServers((current) => [...current, EMPTY_SERVER])}>
              添加
            </Button>
            <Button size='sm' color='primary' startContent={<LuSave />} isLoading={saving} onPress={() => void save()}>
              保存
            </Button>
            <Button size='sm' variant='flat' startContent={<LuRefreshCw />} onPress={() => void load()}>
              刷新
            </Button>
          </div>
        )}
      />
      {configPath && (
        <div className='mb-3 text-xs text-default-400'>
          配置路径: {configPath}
        </div>
      )}
      <WarningList warnings={warnings} />
      <FilterToolbar
        query={query}
        onQueryChange={setQuery}
        tags={tags}
        activeTag={tag}
        onTagChange={setTag}
        placeholder='搜索 MCP 名称、URL、transport 或 tool'
        total={servers.length}
        shown={filteredServers.length}
      />
      <div className='mt-3 grid grid-cols-1 gap-3 xl:grid-cols-2'>
        {filteredServers.map((server) => {
          const index = servers.indexOf(server);
          return (
          <div key={`${server.name || 'new'}-${index}`} className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
            <div className='mb-3 flex items-center justify-between gap-3'>
              <div className='flex min-w-0 items-center gap-2'>
                <Switch
                  size='sm'
                  isSelected={server.active}
                  onValueChange={(active) => updateServer(index, (item) => ({ ...item, active }))}
                />
                <span className='truncate text-sm font-semibold text-default-700 dark:text-default-100'>
                  {server.name || '新 MCP Server'}
                </span>
              </div>
              <Button
                isIconOnly
                size='sm'
                variant='light'
                color='danger'
                onPress={() => setServers((current) => current.filter((_, currentIndex) => currentIndex !== index))}
              >
                <LuTrash2 />
              </Button>
            </div>
            <div className='grid gap-3'>
              <Input
                size='sm'
                label='name'
                variant='bordered'
                value={server.name}
                onChange={(event) => updateServer(index, (item) => ({ ...item, name: event.target.value }))}
              />
              <Input
                size='sm'
                label='transport'
                variant='bordered'
                value={server.transport}
                onChange={(event) => updateServer(index, (item) => ({ ...item, transport: event.target.value }))}
              />
              <Input
                size='sm'
                label='url'
                variant='bordered'
                value={server.url}
                onChange={(event) => updateServer(index, (item) => ({ ...item, url: event.target.value }))}
              />
              <div className='flex flex-wrap gap-2'>
                <Chip size='sm' variant='flat' color='primary'>
                  {server.toolCount ?? 0} tools
                </Chip>
                {(server.warnings ?? []).map((warning) => (
                  <Chip key={warning} size='sm' variant='flat' color='warning'>
                    warning
                  </Chip>
                ))}
              </div>
              {(server.toolNames ?? []).length > 0 && (
                <div className='flex flex-wrap gap-1'>
                  {server.toolNames?.map((toolName) => (
                    <Chip key={toolName} size='sm' variant='flat'>
                      {toolName}
                    </Chip>
                  ))}
                </div>
              )}
              <Button size='sm' variant='flat' onPress={() => void test(server)}>
                测试
              </Button>
            </div>
          </div>
          );
        })}
        {!filteredServers.length && !loading && (
          <div className='rounded-xl border border-dashed border-white/25 p-6 text-center text-sm text-default-400 dark:border-white/10'>
            {servers.length ? '没有匹配的 MCP Server' : '暂无 MCP Server'}
          </div>
        )}
      </div>
    </div>
  );
}

function SkillCard ({
  skill,
  selected,
  onRead,
}: {
  skill: SkillInventoryItem;
  selected: boolean;
  onRead: (name: string) => void;
}) {
  return (
    <button
      type='button'
      className={`w-full rounded-xl border p-3 text-left transition ${selected ? 'border-primary/40 bg-primary/10' : 'border-white/20 bg-white/45 hover:bg-white/65 dark:border-white/10 dark:bg-white/5 dark:hover:bg-white/10'}`}
      onClick={() => onRead(skill.name)}
    >
      <div className='truncate text-sm font-semibold text-default-800 dark:text-default-100'>
        {skill.name}
      </div>
      <div className='mt-1 text-xs text-default-500'>
        {skill.description || '暂无描述'}
      </div>
      {skill.path && (
        <div className='mt-2 truncate text-xs text-default-400'>
          {skill.path}
        </div>
      )}
    </button>
  );
}

function SkillsPanel () {
  const [loading, setLoading] = useState(true);
  const [reading, setReading] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [skills, setSkills] = useState<SkillInventoryItem[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [managedRoot, setManagedRoot] = useState('');
  const [selected, setSelected] = useState<SkillReadResponse | null>(null);
  const [uploadName, setUploadName] = useState('');
  const [uploadContent, setUploadContent] = useState('');
  const [overwrite, setOverwrite] = useState(false);
  const [query, setQuery] = useState('');
  const importInputRef = useRef<HTMLInputElement>(null);
  const folderInputRef = useRef<HTMLInputElement>(null);

  const load = async () => {
    setLoading(true);
    try {
      const data = await CapabilityManager.getSkills();
      setSkills(data.skills);
      setWarnings(data.warnings ?? []);
      setManagedRoot(data.managedRoot ?? '');
    } catch (error) {
      toast.error(`加载 Skills 失败: ${(error as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const read = async (name: string) => {
    setReading(true);
    try {
      const data = await CapabilityManager.readSkill(name);
      setSelected(data);
    } catch (error) {
      toast.error(`读取 Skill 失败: ${(error as Error).message}`);
    } finally {
      setReading(false);
    }
  };

  const importFiles = async (files: File[]) => {
    if (!files.length) {
      return;
    }
    setUploading(true);
    const loadingToast = toast.loading('正在导入 Skill...');
    try {
      const data = await CapabilityManager.importSkills(files, overwrite);
      setManagedRoot(data.managedRoot ?? managedRoot);
      toast.success(`已导入 ${data.count} 个 Skill`, { id: loadingToast });
      await load();
      const firstSkill = data.skills[0]?.name;
      if (firstSkill) {
        await read(firstSkill);
      }
    } catch (error) {
      toast.error(`导入 Skill 失败: ${(error as Error).message}`, { id: loadingToast });
    } finally {
      setUploading(false);
    }
  };

  const handleImportChange = async (event: ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.target.files ?? []);
    event.target.value = '';
    await importFiles(files);
  };

  const upload = async () => {
    if (!uploadName.trim() || !uploadContent.trim()) {
      toast.error('请填写名称和内容');
      return;
    }
    setUploading(true);
    try {
      const data = await CapabilityManager.uploadSkill({
        name: uploadName,
        content: uploadContent,
        overwrite,
      });
      toast.success('Skill 已上传');
      setUploadName('');
      setUploadContent('');
      await load();
      await read(data.name);
    } catch (error) {
      toast.error(`上传 Skill 失败: ${(error as Error).message}`);
    } finally {
      setUploading(false);
    }
  };

  const selectedName = selected?.name;
  const selectedExists = useMemo(
    () => selectedName ? skills.some((skill) => skill.name === selectedName) : false,
    [selectedName, skills]
  );
  const filteredSkills = useMemo(
    () => skills.filter((skill) => matchesQuery(query, [
      skill.name,
      skill.description,
      skill.path,
    ])),
    [query, skills]
  );

  return (
    <div className='relative'>
      <PageLoading loading={loading || reading} />
      <SectionTitle
        icon={<LuBookOpen />}
        title='Skills'
        action={(
          <Button size='sm' variant='flat' startContent={<LuRefreshCw />} onPress={() => void load()}>
            刷新
          </Button>
        )}
      />
      <WarningList warnings={warnings} />
      <FilterToolbar
        query={query}
        onQueryChange={setQuery}
        placeholder='搜索 Skill 名称、描述或路径'
        total={skills.length}
        shown={filteredSkills.length}
      />
      <div className='mt-3 grid min-h-[32rem] grid-cols-1 gap-3 xl:grid-cols-[20rem_minmax(0,1fr)_22rem]'>
        <div className='space-y-2 overflow-y-auto'>
          {filteredSkills.map((skill) => (
            <SkillCard
              key={skill.name}
              skill={skill}
              selected={skill.name === selectedName}
              onRead={(name) => void read(name)}
            />
          ))}
          {!filteredSkills.length && !loading && (
            <div className='rounded-xl border border-dashed border-white/25 p-6 text-center text-sm text-default-400 dark:border-white/10'>
              {skills.length ? '没有匹配的 Skill' : '暂无 Skill'}
            </div>
          )}
        </div>
        <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
          <div className='mb-3 flex items-center justify-between gap-2'>
            <div className='min-w-0'>
              <div className='truncate text-sm font-semibold text-default-700 dark:text-default-100'>
                {selected?.name || '选择一个 Skill'}
              </div>
              {selected?.path && (
                <div className='truncate text-xs text-default-400'>
                  {selected.path}
                </div>
              )}
            </div>
            {selected?.truncated && (
              <Chip size='sm' color='warning' variant='flat'>
                truncated
              </Chip>
            )}
            {selectedName && !selectedExists && (
              <Chip size='sm' color='danger' variant='flat'>
                missing
              </Chip>
            )}
          </div>
          <Textarea
            minRows={18}
            value={selected?.content ?? ''}
            readOnly
            variant='bordered'
            placeholder='Skill 内容会显示在这里'
          />
        </div>
        <div className='rounded-xl border border-white/20 bg-white/45 p-3 dark:border-white/10 dark:bg-white/5'>
          <SectionTitle icon={<LuUpload />} title='导入 Skill' />
          <div className='grid gap-3'>
            {managedRoot && (
              <div className='rounded-xl border border-white/15 bg-white/40 p-3 text-xs text-default-500 dark:border-white/10 dark:bg-black/10 dark:text-default-400'>
                存储目录: {managedRoot}
              </div>
            )}
            <div className='grid gap-2 sm:grid-cols-2'>
              <Button
                variant='flat'
                startContent={<LuFileArchive />}
                isLoading={uploading}
                onPress={() => importInputRef.current?.click()}
              >
                文件/压缩包
              </Button>
              <Button
                variant='flat'
                startContent={<LuFolderOpen />}
                isLoading={uploading}
                onPress={() => folderInputRef.current?.click()}
              >
                文件夹
              </Button>
            </div>
            <input
              ref={importInputRef}
              type='file'
              multiple
              accept='.md,.markdown,.zip,.rar,.7z'
              className='hidden'
              onChange={(event) => void handleImportChange(event)}
            />
            <input
              ref={folderInputRef}
              type='file'
              multiple
              className='hidden'
              {...({ webkitdirectory: '', directory: '' } as Record<string, string>)}
              onChange={(event) => void handleImportChange(event)}
            />
            <div className='rounded-xl border border-dashed border-white/20 p-3 text-xs leading-5 text-default-500 dark:border-white/10'>
              支持导入单个 `SKILL.md`、技能文件夹，以及 `.zip` / `.rar` / `.7z` 压缩包。
              目录或压缩包内允许 1 到 3 层嵌套，系统会自动搜索并安装到用户 Skill 目录。
            </div>
            <Switch size='sm' isSelected={overwrite} onValueChange={setOverwrite}>
              覆盖同名 Skill
            </Switch>
            <div className='rounded-xl border border-white/15 bg-white/35 p-3 dark:border-white/10 dark:bg-black/10'>
              <div className='mb-3 flex items-center gap-2 text-sm font-semibold text-default-700 dark:text-default-100'>
                <LuFileText />
                手动创建
              </div>
              <div className='grid gap-3'>
                <Input
                  label='name'
                  variant='bordered'
                  value={uploadName}
                  onChange={(event) => setUploadName(event.target.value)}
                />
                <Textarea
                  label='content'
                  variant='bordered'
                  minRows={10}
                  value={uploadContent}
                  onChange={(event) => setUploadContent(event.target.value)}
                />
                <Button color='primary' startContent={<LuUpload />} isLoading={uploading} onPress={() => void upload()}>
                  上传
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function CapabilitiesPage () {
  return (
    <>
      <title>能力面板 - Liteyuki WebUI</title>
      <div className='p-2 md:p-4'>
        <div className='mb-4 flex items-center gap-2 text-default-700 dark:text-default-100'>
          <LuBraces size={24} />
          <h1 className='text-2xl font-bold'>能力面板</h1>
        </div>
        <div className='rounded-2xl border border-white/20 bg-white/55 p-3 backdrop-blur-xl dark:border-white/10 dark:bg-black/35'>
          <Tabs
            aria-label='AI Capabilities'
            classNames={{
              tabList: 'bg-white/40 dark:bg-black/20 backdrop-blur-md',
              cursor: 'bg-white/80 dark:bg-white/10 backdrop-blur-md shadow-sm',
              panel: 'pt-4',
            }}
          >
            <Tab key='tools' title='Tools'>
              <ToolsPanel />
            </Tab>
            <Tab key='mcp' title='MCP'>
              <McpPanel />
            </Tab>
            <Tab key='skills' title='Skills'>
              <SkillsPanel />
            </Tab>
          </Tabs>
        </div>
      </div>
    </>
  );
}
