<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle">
    <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
    <div v-else-if="pageError" class="empty-state danger">{{ pageError }}</div>
    <div v-else class="team-stack">
      <section class="panel">
        <div class="panel-header">
          <div>
            <h3>{{ labels.workspaces }}</h3>
            <p class="muted">{{ labels.workspaceDesc }}</p>
          </div>
          <span class="badge badge-accent">{{ workspaces.length }}</span>
        </div>
        <div class="workspace-grid">
          <article v-for="workspace in workspaces" :key="workspace.uid" :class="['workspace-card', { active: workspace.selected }]">
            <strong>{{ workspace.name }}</strong>
            <span class="mono">{{ workspace.uid }}</span>
            <div class="workspace-meta">
              <span>{{ roleText(workspace.role) }}</span>
              <span>{{ workspace.member_count }} {{ labels.members }}</span>
              <span>{{ workspace.project_count }} {{ labels.projects }}</span>
            </div>
          </article>
        </div>
        <form class="inline-form" @submit.prevent="createWorkspace">
          <div class="form-group">
            <label for="workspace-name">{{ labels.newWorkspace }}</label>
            <input id="workspace-name" v-model.trim="newWorkspaceName" maxlength="50" required :placeholder="labels.workspacePlaceholder" />
          </div>
          <button class="btn btn-primary" type="submit" :disabled="savingWorkspace">{{ savingWorkspace ? labels.saving : labels.create }}</button>
        </form>
        <form v-if="selectedWorkspace && isAdmin" class="inline-form" @submit.prevent="renameWorkspace">
          <div class="form-group">
            <label for="workspace-rename">{{ labels.renameWorkspace }}</label>
            <input id="workspace-rename" v-model.trim="workspaceName" maxlength="50" required />
          </div>
          <button class="btn btn-ghost" type="submit" :disabled="savingWorkspace">{{ labels.save }}</button>
        </form>
      </section>

      <section class="panel">
        <div class="panel-header">
          <div>
            <h3>{{ labels.members }}</h3>
            <p class="muted">{{ isAdmin ? labels.memberAdminDesc : labels.memberReadDesc }}</p>
          </div>
          <span class="badge badge-accent">{{ members.length }}</span>
        </div>
        <form v-if="isAdmin" class="member-form" @submit.prevent="addMember">
          <div class="form-group">
            <label for="member-email">{{ t('email') }}</label>
            <input id="member-email" v-model.trim="newMemberEmail" type="email" required :placeholder="labels.registeredEmail" />
          </div>
          <div class="form-group">
            <label for="member-role">{{ labels.role }}</label>
            <select id="member-role" v-model="newMemberRole">
              <option value="member">{{ labels.member }}</option>
              <option value="admin">{{ labels.admin }}</option>
            </select>
          </div>
          <button class="btn btn-primary" type="submit" :disabled="savingMember">{{ savingMember ? labels.saving : labels.addMember }}</button>
        </form>
        <div v-if="!members.length" class="empty-state">{{ labels.noMembers }}</div>
        <div v-else class="table-wrap">
          <table class="data-table">
            <thead><tr><th>{{ labels.name }}</th><th>{{ t('email') }}</th><th>{{ labels.role }}</th><th>{{ labels.joined }}</th><th>{{ t('actions') }}</th></tr></thead>
            <tbody>
              <tr v-for="member in members" :key="member.uid">
                <td>{{ member.nickname }} <span v-if="member.is_owner" class="badge badge-accent">{{ labels.owner }}</span></td>
                <td>{{ member.email }}</td>
                <td>
                  <select v-if="isAdmin && !member.is_owner" v-model="member.role" @change="updateMemberRole(member)">
                    <option value="member">{{ labels.member }}</option>
                    <option value="admin">{{ labels.admin }}</option>
                  </select>
                  <span v-else>{{ roleText(member.role) }}</span>
                </td>
                <td>{{ formatTime(member.created_at) }}</td>
                <td>
                  <button v-if="isAdmin && !member.is_owner && member.uid !== profileUid" class="btn btn-danger btn-sm" type="button" @click="removeMember(member)">
                    {{ labels.remove }}
                  </button>
                  <span v-else>--</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <section class="panel">
        <div class="panel-header">
          <div>
            <h3>{{ labels.projects }}</h3>
            <p class="muted">{{ labels.projectDesc }}</p>
          </div>
          <span class="badge badge-accent">{{ projects.length }}</span>
        </div>
        <form v-if="isAdmin" class="inline-form" @submit.prevent="createProject">
          <div class="form-group">
            <label for="project-name">{{ labels.newProject }}</label>
            <input id="project-name" v-model.trim="newProjectName" maxlength="50" required :placeholder="labels.projectPlaceholder" />
          </div>
          <button class="btn btn-primary" type="submit" :disabled="savingProject">{{ savingProject ? labels.saving : labels.create }}</button>
        </form>
        <div v-if="!projects.length" class="empty-state">{{ labels.noProjects }}</div>
        <div v-else class="project-grid">
          <article v-for="project in projects" :key="project.uid" class="project-card">
            <div>
              <input v-if="isAdmin && !project.is_default" v-model.trim="project.name" maxlength="50" />
              <strong v-else>{{ project.name }}</strong>
              <span class="mono">{{ project.uid }}</span>
            </div>
            <div v-if="isAdmin && !project.is_default" class="table-actions">
              <button class="btn btn-ghost btn-sm" type="button" @click="renameProject(project)">{{ labels.save }}</button>
              <button class="btn btn-danger btn-sm" type="button" @click="disableProject(project)">{{ labels.disable }}</button>
            </div>
            <span v-else-if="project.is_default" class="badge badge-accent">{{ labels.defaultProject }}</span>
          </article>
        </div>
      </section>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders, getSelectedWorkspace, setSelectedWorkspace } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const { t, isZh } = useI18n()
const loading = ref(true)
const pageError = ref('')
const workspaces = ref([])
const members = ref([])
const projects = ref([])
const profileUid = ref('')
const newWorkspaceName = ref('')
const workspaceName = ref('')
const newMemberEmail = ref('')
const newMemberRole = ref('member')
const newProjectName = ref('')
const savingWorkspace = ref(false)
const savingMember = ref(false)
const savingProject = ref(false)

const labels = computed(() => isZh.value ? {
  title: '团队管理', subtitle: '管理工作空间、成员角色和项目。', workspaces: '工作空间', workspaceDesc: '余额、密钥、日志和供应商凭据都按工作空间隔离。',
  members: '成员', projects: '项目', newWorkspace: '新工作空间名称', workspacePlaceholder: '例如：研发团队', renameWorkspace: '当前工作空间名称',
  create: '创建', save: '保存', saving: '保存中', role: '角色', member: '成员', admin: '管理员', owner: '所有者', addMember: '添加已有用户',
  registeredEmail: '输入已注册用户邮箱', memberAdminDesc: '管理员可以添加成员、调整角色和移除成员。', memberReadDesc: '普通成员只能查看团队名单。',
  noMembers: '当前没有成员。', name: '名称', joined: '加入时间', remove: '移除', projectDesc: '项目用于划分密钥和调用账单；默认项目不能停用。',
  newProject: '新项目名称', projectPlaceholder: '例如：生产环境', noProjects: '当前没有项目。', disable: '停用', defaultProject: '默认项目',
  loadFailed: '团队数据加载失败。', workspaceCreated: '工作空间已创建。', workspaceUpdated: '工作空间已更新。', memberSaved: '成员已保存。',
  memberUpdated: '成员角色已更新。', memberRemoved: '成员已移除。', projectCreated: '项目已创建。', projectUpdated: '项目已更新。', projectDisabled: '项目已停用。',
  confirmRemove: '确认移除该成员？', confirmDisableProject: '确认停用该项目？',
} : {
  title: 'Team', subtitle: 'Manage workspaces, member roles and projects.', workspaces: 'Workspaces', workspaceDesc: 'Balance, keys, logs and provider credentials are isolated by workspace.',
  members: 'Members', projects: 'Projects', newWorkspace: 'New workspace name', workspacePlaceholder: 'Example: Engineering', renameWorkspace: 'Current workspace name',
  create: 'Create', save: 'Save', saving: 'Saving', role: 'Role', member: 'Member', admin: 'Administrator', owner: 'Owner', addMember: 'Add registered user',
  registeredEmail: 'Enter a registered user email', memberAdminDesc: 'Administrators can add members, change roles and remove members.', memberReadDesc: 'Members can only view the team list.',
  noMembers: 'No members in this workspace.', name: 'Name', joined: 'Joined', remove: 'Remove', projectDesc: 'Projects divide keys and request billing. The Default project cannot be disabled.',
  newProject: 'New project name', projectPlaceholder: 'Example: Production', noProjects: 'No projects in this workspace.', disable: 'Disable', defaultProject: 'Default project',
  loadFailed: 'Failed to load team data.', workspaceCreated: 'Workspace created.', workspaceUpdated: 'Workspace updated.', memberSaved: 'Member saved.',
  memberUpdated: 'Member role updated.', memberRemoved: 'Member removed.', projectCreated: 'Project created.', projectUpdated: 'Project updated.', projectDisabled: 'Project disabled.',
  confirmRemove: 'Remove this member?', confirmDisableProject: 'Disable this project?',
})

const selectedWorkspace = computed(() => workspaces.value.find((workspace) => workspace.selected) || null)
const selectedUid = computed(() => selectedWorkspace.value?.uid || getSelectedWorkspace())
const isAdmin = computed(() => selectedWorkspace.value?.role === 'admin')

const requireSuccess = (data) => {
  if (data.code !== 0) throw new Error(data.message || labels.value.loadFailed)
  return data
}

const loadAll = async () => {
  loading.value = true
  pageError.value = ''
  try {
    const [profileData, workspaceData] = await Promise.all([
      requestJson('/api/auth/me', { headers: authHeaders() }),
      requestJson('/api/workspaces', { headers: authHeaders() }),
    ])
    requireSuccess(profileData)
    requireSuccess(workspaceData)
    profileUid.value = profileData.data?.uid || ''
    workspaces.value = workspaceData.data?.items || []
    const current = workspaces.value.find((workspace) => workspace.selected)
    if (current?.uid && getSelectedWorkspace() !== current.uid) setSelectedWorkspace(current.uid)
    workspaceName.value = current?.name || ''
    if (!current?.uid) throw new Error(labels.value.loadFailed)
    const [memberData, projectData] = await Promise.all([
      requestJson(`/api/workspaces/${encodeURIComponent(current.uid)}/members`, { headers: authHeaders() }),
      requestJson(`/api/workspaces/${encodeURIComponent(current.uid)}/projects`, { headers: authHeaders() }),
    ])
    requireSuccess(memberData)
    requireSuccess(projectData)
    members.value = memberData.data?.items || []
    projects.value = projectData.data?.items || []
  } catch (error) {
    pageError.value = apiErrorMessage(error, error?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
}

const createWorkspace = async () => {
  savingWorkspace.value = true
  try {
    const data = requireSuccess(await requestJson('/api/workspaces', {
      method: 'POST', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify({ name: newWorkspaceName.value }),
    }))
    setSelectedWorkspace(data.data.uid)
    notifySuccess(labels.value.workspaceCreated)
    window.location.reload()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed) }
  finally { savingWorkspace.value = false }
}

const renameWorkspace = async () => {
  savingWorkspace.value = true
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}`, {
      method: 'PUT', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify({ name: workspaceName.value }),
    }))
    notifySuccess(labels.value.workspaceUpdated)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed) }
  finally { savingWorkspace.value = false }
}

const addMember = async () => {
  savingMember.value = true
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}/members`, {
      method: 'POST', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify({ email: newMemberEmail.value, role: newMemberRole.value }),
    }))
    newMemberEmail.value = ''
    notifySuccess(labels.value.memberSaved)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed) }
  finally { savingMember.value = false }
}

const updateMemberRole = async (member) => {
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}/members/${encodeURIComponent(member.uid)}`, {
      method: 'PUT', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify({ role: member.role }),
    }))
    notifySuccess(labels.value.memberUpdated)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed); await loadAll() }
}

const removeMember = async (member) => {
  if (!window.confirm(labels.value.confirmRemove)) return
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}/members/${encodeURIComponent(member.uid)}`, {
      method: 'DELETE', headers: authHeaders(),
    }))
    notifySuccess(labels.value.memberRemoved)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed) }
}

const createProject = async () => {
  savingProject.value = true
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}/projects`, {
      method: 'POST', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify({ name: newProjectName.value }),
    }))
    newProjectName.value = ''
    notifySuccess(labels.value.projectCreated)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed) }
  finally { savingProject.value = false }
}

const renameProject = async (project) => {
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}/projects/${encodeURIComponent(project.uid)}`, {
      method: 'PUT', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify({ name: project.name }),
    }))
    notifySuccess(labels.value.projectUpdated)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed); await loadAll() }
}

const disableProject = async (project) => {
  if (!window.confirm(labels.value.confirmDisableProject)) return
  try {
    requireSuccess(await requestJson(`/api/workspaces/${encodeURIComponent(selectedUid.value)}/projects/${encodeURIComponent(project.uid)}`, {
      method: 'DELETE', headers: authHeaders(),
    }))
    notifySuccess(labels.value.projectDisabled)
    await loadAll()
  } catch (error) { notifyError(error?.message || labels.value.loadFailed) }
}

const roleText = (role) => role === 'admin' ? labels.value.admin : labels.value.member
const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'

onMounted(loadAll)
</script>

<style scoped>
.team-stack { display: grid; gap: 18px; }
.workspace-grid, .project-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 12px; }
.workspace-card, .project-card { border: 1px solid var(--border); border-radius: var(--radius); background: #111; padding: 16px; display: grid; gap: 9px; }
.workspace-card.active { border-color: rgba(77,139,247,.7); box-shadow: inset 0 0 0 1px rgba(77,139,247,.2); }
.workspace-card .mono, .project-card .mono { color: var(--muted); font-size: 12px; }
.workspace-meta { display: flex; flex-wrap: wrap; gap: 12px; color: var(--muted); font-size: 13px; }
.inline-form, .member-form { display: grid; grid-template-columns: minmax(240px, 1fr) auto; gap: 12px; align-items: end; margin-top: 18px; }
.member-form { grid-template-columns: minmax(260px, 1.4fr) minmax(150px, .6fr) auto; }
.project-card { grid-template-columns: minmax(0, 1fr) auto; align-items: center; }
.project-card > div:first-child { display: grid; gap: 8px; }
button:disabled { opacity: .58; cursor: wait; }
@media (max-width: 720px) {
  .inline-form, .member-form, .project-card { grid-template-columns: 1fr; }
}
</style>
