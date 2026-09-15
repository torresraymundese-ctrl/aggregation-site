<template>
  <div ref="hostRef" class="token-scene" aria-label="Token block 3D scene"></div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as THREE from 'three'

const props = defineProps({
  models: { type: Array, default: () => [] },
  selectedId: { type: String, default: '' },
})

const emit = defineEmits(['select', 'open'])

const hostRef = ref(null)
const selectedIdRef = computed(() => props.selectedId)

let renderer
let scene
let camera
let root
let resizeObserver
let animationId
let startTime = 0
let raycaster
let pointer
let dragPlane
let draggedBlock
let hoveredBlock
let dragOffset = new THREE.Vector3()
let pointerStart = { x: 0, y: 0 }
let viewDrag = null
const easedTarget = new THREE.Vector3()
const modelMeshes = new Map()
const modelBlocks = new Map()
const interactiveMeshes = []

const cell = 0.86
const floatBounds = { x: 8.8, y: 4.35, nearZ: 3.4, farZ: -5.6 }
const layouts = [
  [-8.9, 3.15, 0.9, 4, 1, 0.54, -0.48, 0.36, -0.52, 1.24],
  [-5.9, 1.2, -2.6, 1, 1, 0.72, 0.42, -0.24, 0.46, 0.82],
  [-3.6, 3.55, -4.6, 2, 2, 0.52, -0.28, 0.52, 0.18, 0.7],
  [-0.9, 2.15, 0.4, 4, 2, 0.78, 0.18, -0.44, 0.28, 1.08],
  [2.9, 3.72, -3.5, 4, 1, 0.62, -0.36, -0.16, -0.24, 0.86],
  [8.6, 2.55, 1.2, 2, 2, 0.9, 0.28, 0.56, -0.34, 1.4],
  [-7.4, 0.35, -0.6, 4, 2, 0.58, 0.08, 0.18, -0.75, 1.08],
  [-4.1, -0.82, 1.8, 4, 1, 1.0, -0.18, -0.52, 0.2, 1.34],
  [-0.2, -0.35, -2.1, 1, 1, 0.64, 0.34, 0.28, -0.18, 0.66],
  [3.4, 0.68, -4.8, 2, 2, 0.48, -0.24, -0.28, 0.36, 0.58],
  [7.8, -0.45, -1.1, 4, 1, 0.7, 0.18, 0.44, 0.64, 1.1],
  [-8.2, -2.65, -3.2, 2, 2, 0.86, -0.36, 0.14, 0.42, 0.76],
  [-4.8, -3.72, 0.2, 4, 1, 0.52, 0.22, -0.34, -0.3, 1.0],
  [-1.25, -2.15, 2.0, 1, 1, 0.96, -0.18, 0.62, 0.18, 1.46],
  [1.8, -3.5, -1.8, 4, 2, 0.62, 0.28, -0.16, -0.42, 0.84],
  [5.0, -2.45, 0.9, 4, 1, 0.92, -0.22, 0.34, 0.24, 1.28],
  [9.1, -3.25, -2.7, 1, 1, 0.58, 0.32, -0.46, 0.5, 0.72],
]

const palettes = [
  { body: 0x3678d8, side: 0x25528e, stud: 0x5d98eb },
  { body: 0xd5534a, side: 0x96332f, stud: 0xee756e },
  { body: 0xf18a1b, side: 0xb45f10, stud: 0xffa640 },
  { body: 0x28a66f, side: 0x18744b, stud: 0x45c98e },
  { body: 0x8257e5, side: 0x5634a7, stud: 0xa27cff },
  { body: 0xec6fa7, side: 0xae4676, stud: 0xff95c4 },
  { body: 0xd5d7dc, side: 0x9a9fa7, stud: 0xf1f2f4 },
  { body: 0x68707a, side: 0x3f464f, stud: 0x8d949e },
]

const initScene = () => {
  const host = hostRef.value
  if (!host) return

  scene = new THREE.Scene()
  scene.background = null
  camera = new THREE.PerspectiveCamera(46, host.clientWidth / host.clientHeight, 0.1, 100)
  camera.position.set(0, 0.1, 12.4)
  camera.lookAt(0, 0, 0)

  renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    preserveDrawingBuffer: true,
    powerPreference: 'high-performance',
  })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
  renderer.setSize(host.clientWidth, host.clientHeight)
  host.appendChild(renderer.domElement)

  raycaster = new THREE.Raycaster()
  pointer = new THREE.Vector2()
  dragPlane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0)

  root = new THREE.Group()
  root.rotation.x = -0.04
  root.rotation.y = -0.08
  scene.add(root)

  addLights()
  syncBlocks()
  bindEvents()

  startTime = performance.now()
  resizeObserver = new ResizeObserver(resizeScene)
  resizeObserver.observe(host)
  animate()
}

const addLights = () => {
  const hemi = new THREE.HemisphereLight(0xffffff, 0x242a31, 1.35)
  scene.add(hemi)

  const key = new THREE.DirectionalLight(0xffffff, 2.4)
  key.position.set(-5, 9, 8)
  key.castShadow = true
  key.shadow.mapSize.set(1024, 1024)
  scene.add(key)

  const fill = new THREE.DirectionalLight(0x8aa7ff, 0.7)
  fill.position.set(6, 5, -5)
  scene.add(fill)
}

const buildDepthGuides = () => {
  const guideGroup = new THREE.Group()
  const guideMat = new THREE.LineBasicMaterial({
    color: 0x4d8bf7,
    transparent: true,
    opacity: 0.14,
  })
  ;[2.9, 4.5, 6.1].forEach((radius, index) => {
    const curve = new THREE.EllipseCurve(0, 0, radius * 1.35, radius, 0, Math.PI * 2)
    const points = curve.getPoints(96).map((point) => new THREE.Vector3(point.x, point.y, -2.6 - index * 0.3))
    const line = new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), guideMat)
    line.rotation.z = index % 2 ? 0.18 : -0.12
    guideGroup.add(line)
  })
  root.add(guideGroup)
}

const syncBlocks = () => {
  if (!root) return
  for (const block of modelBlocks.values()) {
    root.remove(block)
    disposeObject(block)
  }
  modelMeshes.clear()
  modelBlocks.clear()
  interactiveMeshes.length = 0

  props.models.forEach((model, index) => {
    const [x, y, z, w, h, blockHeight, rotationX, rotationY, rotationZ, scale = 1] = layouts[index % layouts.length]
    const block = createBlock(model, index, w, h, blockHeight)
    const home = new THREE.Vector3(x, y, z)
    const scatter = new THREE.Vector3(
      Math.sin(index * 2.17) * 0.42,
      Math.cos(index * 1.73) * 0.3,
      Math.sin(index * 1.31) * 0.22,
    )
    block.position.copy(home).add(scatter)
    block.rotation.set(rotationX + 0.22, rotationY - 0.18, rotationZ + 0.16)
    block.userData.grid = { x, y, z, w, h }
    block.userData.targetPosition = home
    block.userData.targetRotation = new THREE.Euler(rotationX, rotationY, rotationZ)
    block.userData.baseScale = scale
    block.userData.floating = true
    block.userData.floatPhase = index * 0.74
    block.userData.floatAmp = 0.18 + (index % 5) * 0.045
    block.userData.sideAmp = 0.16 + (index % 4) * 0.05
    block.userData.depthAmp = 0.1 + (index % 3) * 0.05
    block.userData.speed = 0.48 + (index % 6) * 0.08
    block.userData.spinAmp = new THREE.Vector3(
      0.18 + (index % 3) * 0.05,
      0.22 + (index % 4) * 0.04,
      0.16 + (index % 5) * 0.035,
    )
    block.userData.selected = model.model_id === selectedIdRef.value
    block.scale.setScalar(scale * 0.92)
    modelBlocks.set(model.model_id, block)
    root.add(block)
  })

  highlightSelected()
}

const createBlock = (model, index, w, h, blockHeight) => {
  const palette = palettes[index % palettes.length]
  const block = new THREE.Group()
  block.userData.model = model
  const registerMesh = (mesh) => {
    mesh.userData.block = block
    modelMeshes.set(mesh.uuid, block)
    interactiveMeshes.push(mesh)
  }

  const width = w * cell - 0.12
  const depth = h * cell - 0.12
  const height = blockHeight
  const bodyMat = new THREE.MeshStandardMaterial({
    color: palette.body,
    roughness: 0.76,
    metalness: 0.02,
  })
  const body = new THREE.Mesh(new THREE.BoxGeometry(width, height, depth), bodyMat)
  body.position.y = height / 2
  body.castShadow = true
  body.receiveShadow = true
  block.add(body)
  registerMesh(body)

  const sideMat = new THREE.MeshStandardMaterial({
    color: palette.side,
    roughness: 0.8,
    metalness: 0.02,
  })
  const side = new THREE.Mesh(new THREE.BoxGeometry(width * 0.96, Math.max(0.08, height * 0.18), depth * 0.94), sideMat)
  side.position.y = 0.05
  side.castShadow = true
  block.add(side)
  registerMesh(side)

  const studGeo = new THREE.CylinderGeometry(0.17, 0.18, 0.14, 24)
  const studMat = new THREE.MeshStandardMaterial({
    color: palette.stud,
    roughness: 0.72,
    metalness: 0.02,
  })
  for (let ix = 0; ix < w; ix += 1) {
    for (let iz = 0; iz < h; iz += 1) {
      const stud = new THREE.Mesh(studGeo, studMat)
      stud.position.set((ix - (w - 1) / 2) * cell, height + 0.08, (iz - (h - 1) / 2) * cell)
      stud.castShadow = true
      block.add(stud)
      registerMesh(stud)
    }
  }

  const label = makeLabelSprite(displayModelName(model), providerShort(model.provider), width)
  label.position.set(0, height + 0.62, 0)
  label.visible = false
  block.userData.label = label
  block.add(label)

  return block
}

const makeLabelSprite = (name, provider, blockWidth) => {
  const canvas = document.createElement('canvas')
  canvas.width = 512
  canvas.height = 160
  const ctx = canvas.getContext('2d')
  ctx.clearRect(0, 0, canvas.width, canvas.height)
  ctx.fillStyle = 'rgba(9, 13, 20, .78)'
  roundRect(ctx, 42, 30, 428, 100, 28)
  ctx.fill()
  ctx.strokeStyle = 'rgba(255,255,255,.12)'
  ctx.lineWidth = 2
  roundRect(ctx, 42, 30, 428, 100, 28)
  ctx.stroke()
  ctx.fillStyle = 'rgba(255,255,255,.86)'
  ctx.font = '750 34px Inter, HarmonyOS Sans SC, Microsoft YaHei, Segoe UI, sans-serif'
  ctx.textAlign = 'center'
  ctx.fillText(trimText(name, 18), 256, 78)
  ctx.fillStyle = 'rgba(255,255,255,.68)'
  ctx.font = '700 20px ui-monospace, Consolas, monospace'
  ctx.fillText(provider, 256, 108)

  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  texture.anisotropy = Math.min(8, renderer?.capabilities?.getMaxAnisotropy?.() || 1)
  const width = Math.max(1.55, Math.min(3.25, blockWidth * 1.08))
  const height = width * (canvas.height / canvas.width)
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: texture,
      transparent: true,
      opacity: 0,
      depthTest: false,
      depthWrite: false,
    }),
  )
  sprite.scale.set(width, height, 1)
  sprite.renderOrder = 20
  return sprite
}

const roundRect = (ctx, x, y, w, h, r) => {
  ctx.beginPath()
  ctx.moveTo(x + r, y)
  ctx.lineTo(x + w - r, y)
  ctx.quadraticCurveTo(x + w, y, x + w, y + r)
  ctx.lineTo(x + w, y + h - r)
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h)
  ctx.lineTo(x + r, y + h)
  ctx.quadraticCurveTo(x, y + h, x, y + h - r)
  ctx.lineTo(x, y + r)
  ctx.quadraticCurveTo(x, y, x + r, y)
  ctx.closePath()
}

const bindEvents = () => {
  const canvas = renderer.domElement
  canvas.addEventListener('pointerdown', onPointerDown)
  canvas.addEventListener('pointermove', onPointerMove)
  canvas.addEventListener('pointerup', onPointerUp)
  canvas.addEventListener('pointercancel', onPointerUp)
  canvas.addEventListener('wheel', onWheel, { passive: false })
  canvas.addEventListener('dblclick', onDoubleClick)
  canvas.addEventListener('pointerleave', onPointerLeave)
  canvas.addEventListener('contextmenu', (event) => event.preventDefault())
}

const removeEvents = () => {
  const canvas = renderer?.domElement
  if (!canvas) return
  canvas.removeEventListener('pointerdown', onPointerDown)
  canvas.removeEventListener('pointermove', onPointerMove)
  canvas.removeEventListener('pointerup', onPointerUp)
  canvas.removeEventListener('pointercancel', onPointerUp)
  canvas.removeEventListener('wheel', onWheel)
  canvas.removeEventListener('dblclick', onDoubleClick)
  canvas.removeEventListener('pointerleave', onPointerLeave)
}

const setPointer = (event) => {
  const rect = renderer.domElement.getBoundingClientRect()
  pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1
  pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1
  raycaster.setFromCamera(pointer, camera)
}

const getBlockAtPointer = (event) => {
  setPointer(event)
  const hit = raycaster.intersectObjects(interactiveMeshes, false)[0]
  return hit?.object?.userData?.block || null
}

const onPointerDown = (event) => {
  pointerStart = { x: event.clientX, y: event.clientY }
  const block = getBlockAtPointer(event)
  if (block && event.button !== 2) {
    draggedBlock = block
    setDragPlane(block)
    const point = intersectDragPlane(event)
    const blockWorld = block.getWorldPosition(new THREE.Vector3())
    dragOffset.copy(point).sub(blockWorld)
    block.userData.dragging = true
    block.userData.floating = false
    block.userData.targetRotation = new THREE.Euler(0.08, block.rotation.y, block.rotation.z)
    root.remove(block)
    root.add(block)
    renderer.domElement.style.cursor = 'grabbing'
    renderer.domElement.setPointerCapture?.(event.pointerId)
    return
  }

  viewDrag = {
    button: event.button,
    x: event.clientX,
    y: event.clientY,
    rootX: root.rotation.x,
    rootY: root.rotation.y,
  }
  renderer.domElement.setPointerCapture?.(event.pointerId)
}

const onPointerMove = (event) => {
  if (draggedBlock) {
    const point = intersectDragPlane(event)
    const worldTarget = point.sub(dragOffset)
    const next = root.worldToLocal(worldTarget)
    draggedBlock.userData.targetPosition.set(
      clamp(next.x, -floatBounds.x, floatBounds.x),
      clamp(next.y, -floatBounds.y, floatBounds.y),
      clamp(next.z + 0.75, floatBounds.farZ, floatBounds.nearZ),
    )
    dispatchSceneEvent('token-block-drag', draggedBlock)
    return
  }

  if (viewDrag) {
    const dx = event.clientX - viewDrag.x
    const dy = event.clientY - viewDrag.y
    root.rotation.y = viewDrag.rootY + dx * 0.004
    root.rotation.x = clamp(viewDrag.rootX + dy * 0.003, -0.34, 0.26)
    return
  }

  const block = getBlockAtPointer(event)
  if (block !== hoveredBlock) {
    if (hoveredBlock) hoveredBlock.userData.hovered = false
    hoveredBlock = block
    if (hoveredBlock) hoveredBlock.userData.hovered = true
    renderer.domElement.style.cursor = block ? 'grab' : 'default'
  }
}

const onPointerUp = (event) => {
  if (draggedBlock) {
    const clickDistance = Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y)
    const clickedModel = clickDistance < 8 ? draggedBlock.userData.model : null
    const target = draggedBlock.userData.targetPosition
    target.set(
      clamp(target.x, -floatBounds.x, floatBounds.x),
      clamp(target.y, -floatBounds.y, floatBounds.y),
      clamp(target.z, floatBounds.farZ, floatBounds.nearZ),
    )
    draggedBlock.userData.dragging = false
    draggedBlock.userData.floating = true
    dispatchSceneEvent('token-block-drop', draggedBlock)
    draggedBlock = null
    if (clickedModel) emit('select', clickedModel)
    renderer.domElement.style.cursor = hoveredBlock ? 'grab' : 'default'
  }
  viewDrag = null
  renderer.domElement.releasePointerCapture?.(event.pointerId)
}

const onPointerLeave = () => {
  if (hoveredBlock && hoveredBlock !== draggedBlock) hoveredBlock.userData.hovered = false
  hoveredBlock = null
}

const onDoubleClick = (event) => {
  const block = getBlockAtPointer(event)
  if (block) emit('open', block.userData.model)
}

const onWheel = (event) => {
  event.preventDefault()
  camera.position.z = clamp(camera.position.z + event.deltaY * 0.006, 8.8, 17)
  camera.lookAt(0, 0, 0)
}

const setDragPlane = (block) => {
  const normal = camera.getWorldDirection(new THREE.Vector3()).normalize()
  const point = block.getWorldPosition(new THREE.Vector3()).add(normal.clone().multiplyScalar(-0.9))
  dragPlane.setFromNormalAndCoplanarPoint(normal, point)
}

const intersectDragPlane = (event) => {
  setPointer(event)
  const point = new THREE.Vector3()
  if (!raycaster.ray.intersectPlane(dragPlane, point)) return new THREE.Vector3()
  return point
}

const clamp = (value, min, max) => Math.min(max, Math.max(min, value))

const dispatchSceneEvent = (name, block) => {
  const position = block.userData.targetPosition || block.position
  hostRef.value?.dispatchEvent(new CustomEvent(name, {
    detail: {
      model_id: block.userData.model.model_id,
      x: Number(position.x.toFixed(2)),
      y: Number(position.y.toFixed(2)),
      z: Number(position.z.toFixed(2)),
    },
  }))
}

const highlightSelected = () => {
  for (const block of modelBlocks.values()) {
    const selected = block.userData.model.model_id === selectedIdRef.value
    block.userData.selected = selected
  }
}

const updateBlocks = (elapsed) => {
  for (const block of modelBlocks.values()) {
    const target = block.userData.targetPosition
    if (!target) continue

    const isDragging = block === draggedBlock
    const isHovered = block.userData.hovered && !isDragging
    const speed = block.userData.speed || 0.6
    const bobX = block.userData.floating
      ? Math.sin(elapsed * speed * 0.77 + block.userData.floatPhase * 1.7) * block.userData.sideAmp
      : 0
    const bobY = block.userData.floating
      ? Math.sin(elapsed * speed + block.userData.floatPhase) * block.userData.floatAmp
      : 0
    const bobZ = block.userData.floating
      ? Math.cos(elapsed * speed * 0.63 + block.userData.floatPhase * 1.3) * block.userData.depthAmp
      : 0
    const hoverLift = isHovered ? 0.16 : 0
    easedTarget.set(target.x + bobX, target.y + bobY + hoverLift, target.z + bobZ)
    block.position.lerp(easedTarget, isDragging ? 0.34 : 0.045)

    const rotationTarget = block.userData.targetRotation || new THREE.Euler()
    const spin = block.userData.spinAmp || new THREE.Vector3(0.16, 0.18, 0.14)
    const floatingRotation = block.userData.floating
      ? {
          x: rotationTarget.x + Math.sin(elapsed * speed * 0.66 + block.userData.floatPhase) * spin.x,
          y: rotationTarget.y + Math.cos(elapsed * speed * 0.51 + block.userData.floatPhase * 1.2) * spin.y,
          z: rotationTarget.z + Math.sin(elapsed * speed * 0.58 + block.userData.floatPhase * 0.8) * spin.z,
        }
      : rotationTarget
    block.rotation.x += (floatingRotation.x - block.rotation.x) * (isDragging ? 0.24 : 0.04)
    block.rotation.y += (floatingRotation.y - block.rotation.y) * (isDragging ? 0.24 : 0.04)
    block.rotation.z += (floatingRotation.z - block.rotation.z) * (isDragging ? 0.24 : 0.04)

    const baseScale = block.userData.baseScale || 1
    const scaleTarget = baseScale * (isDragging ? 1.12 : block.userData.selected ? 1.08 : isHovered ? 1.05 : 1)
    const nextScale = block.scale.x + (scaleTarget - block.scale.x) * 0.12
    block.scale.setScalar(nextScale)

    const label = block.userData.label
    if (label) {
      const showLabel = isHovered || block.userData.selected || isDragging
      label.visible = showLabel || label.material.opacity > 0.02
      label.material.opacity += ((showLabel ? 1 : 0) - label.material.opacity) * 0.18
    }
  }
}

const disposeObject = (object) => {
  object.traverse((item) => {
    item.geometry?.dispose?.()
    if (Array.isArray(item.material)) {
      item.material.forEach((material) => {
        material.map?.dispose?.()
        material.dispose?.()
      })
    } else {
      item.material?.map?.dispose?.()
      item.material?.dispose?.()
    }
  })
}

const resizeScene = () => {
  const host = hostRef.value
  if (!host || !renderer || !camera) return
  const width = host.clientWidth || 800
  const height = host.clientHeight || 520
  renderer.setSize(width, height)
  camera.aspect = width / height
  camera.updateProjectionMatrix()
}

const animate = () => {
  updateBlocks((performance.now() - startTime) / 1000)
  renderer.render(scene, camera)
  animationId = window.requestAnimationFrame(animate)
}

const disposeScene = () => {
  window.cancelAnimationFrame(animationId)
  resizeObserver?.disconnect()
  removeEvents()
  scene?.traverse((object) => {
    object.geometry?.dispose?.()
    if (Array.isArray(object.material)) {
      object.material.forEach((material) => material.dispose?.())
    } else {
      object.material?.map?.dispose?.()
      object.material?.dispose?.()
    }
  })
  renderer?.dispose()
  renderer?.domElement?.remove()
  modelMeshes.clear()
  modelBlocks.clear()
  interactiveMeshes.length = 0
}

const providerShort = (provider) => {
  const value = String(provider || '').toLowerCase()
  if (value.includes('openai')) return 'OA'
  if (value.includes('anthropic') || value.includes('claude')) return 'CL'
  if (value.includes('google') || value.includes('gemini')) return 'GM'
  if (value.includes('volc') || value.includes('火山')) return 'VC'
  if (value.includes('deepseek')) return 'DS'
  if (value.includes('zhipu') || value.includes('glm') || value.includes('智谱')) return 'GL'
  if (value.includes('qwen') || value.includes('dashscope') || value.includes('千问')) return 'QW'
  if (value.includes('moonshot') || value.includes('kimi')) return 'KM'
  if (value.includes('minimax')) return 'MM'
  if (value.includes('stepfun')) return 'SF'
  return 'AI'
}

const displayModelName = (model) => {
  const name = String(model?.display_name || model?.model_id || '')
  return name.replace(/\s+Placeholder$/i, '')
}

const trimText = (value, size) => value.length > size ? `${value.slice(0, size - 1)}...` : value

watch(() => props.models, syncBlocks, { deep: true })
watch(selectedIdRef, highlightSelected)

onMounted(async () => {
  await nextTick()
  initScene()
})

onBeforeUnmount(disposeScene)
</script>
