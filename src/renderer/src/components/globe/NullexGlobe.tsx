import React, { useEffect, useRef } from 'react'
import landMaskUrl from '@renderer/assets/land_mask.png'
import { countryCentroids, detectIsoCodeFromName } from './country-centroids'
import { extractLeadingFlagAndTitle } from '@renderer/components/home/flag-utils'
import type { GlobeRotationState } from './NullexStarfield'

interface GlobePoint {
  x: number
  y: number
  z: number
}

interface MarkerData {
  lat: number
  lon: number
  label: string
  flag: string | null
}

interface NullexGlobeProps {
  activeServerName?: string
  className?: string
  dotColor?: string
  accentColor?: string
  active?: boolean
  rotationRef?: React.RefObject<GlobeRotationState>
}

function hexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace('#', '').trim()
  if (clean.length === 3) {
    const r = parseInt(clean[0] + clean[0], 16)
    const g = parseInt(clean[1] + clean[1], 16)
    const b = parseInt(clean[2] + clean[2], 16)
    if (!Number.isNaN(r) && !Number.isNaN(g) && !Number.isNaN(b)) return [r, g, b]
  }
  if (clean.length === 6) {
    const r = parseInt(clean.slice(0, 2), 16)
    const g = parseInt(clean.slice(2, 4), 16)
    const b = parseInt(clean.slice(4, 6), 16)
    if (!Number.isNaN(r) && !Number.isNaN(g) && !Number.isNaN(b)) return [r, g, b]
  }
  return [229, 229, 229]
}

export const NullexGlobe: React.FC<NullexGlobeProps> = ({
  activeServerName,
  className = '',
  dotColor = '#E5E5E5',
  accentColor = '#E5E5E5',
  active = false,
  rotationRef
}) => {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const pillRef = useRef<HTMLDivElement>(null)
  const markerRef = useRef<MarkerData | null>(null)
  const dotColorRef = useRef<string>(dotColor)
  const accentColorRef = useRef<string>(accentColor)
  const activeRef = useRef<boolean>(active)
  const focusTargetRef = useRef<{ phi: number; theta: number } | null>(null)

  dotColorRef.current = dotColor
  accentColorRef.current = accentColor
  activeRef.current = active

  useEffect(() => {
    if (!activeServerName) {
      markerRef.current = null
      return
    }
    const iso = detectIsoCodeFromName(activeServerName)
    const coords = iso ? countryCentroids[iso] : undefined
    const [lat, lon] = coords ?? [51, 9]
    const parsed = extractLeadingFlagAndTitle(activeServerName)
    markerRef.current = {
      lat,
      lon,
      label: parsed.title.slice(0, 26),
      flag: parsed.flag
    }
    const latRad = (lat * Math.PI) / 180
    const lonRad = (lon * Math.PI) / 180
    focusTargetRef.current = {
      phi: -lonRad,
      theta: Math.max(-0.55, Math.min(0.45, -latRad * 0.45))
    }
  }, [activeServerName])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d', { alpha: true })
    if (!ctx) return

    let animationFrameId = 0
    let points: GlobePoint[] = []

    let phi = 0.45
    let theta = -0.2
    let targetPhi = 0.45
    let targetTheta = -0.2

    let isDragging = false
    let lastMouseX = 0
    let lastMouseY = 0

    // Seeded Fibonacci sampling for uniform sphere distribution (20,000 candidates)
    const initPoints = (maskImage?: HTMLImageElement): void => {
      const candidateCount = 20000
      const goldenAngle = Math.PI * (3 - Math.sqrt(5))
      const newPoints: GlobePoint[] = []

      let maskData: Uint8ClampedArray | null = null
      let mWidth = 0
      let mHeight = 0

      if (maskImage && maskImage.complete && maskImage.naturalWidth > 0) {
        const maskCanvas = document.createElement('canvas')
        mWidth = maskImage.naturalWidth
        mHeight = maskImage.naturalHeight
        maskCanvas.width = mWidth
        maskCanvas.height = mHeight
        const maskCtx = maskCanvas.getContext('2d')
        if (maskCtx) {
          maskCtx.drawImage(maskImage, 0, 0)
          maskData = maskCtx.getImageData(0, 0, mWidth, mHeight).data
        }
      }

      for (let i = 0; i < candidateCount; i++) {
        const y = 1 - (i / (candidateCount - 1)) * 2
        const radiusAtY = Math.sqrt(Math.max(0, 1 - y * y))
        const angle = goldenAngle * i
        const x = Math.cos(angle) * radiusAtY
        const z = Math.sin(angle) * radiusAtY

        // Deterministic pseudo-random threshold per candidate index
        const hash = Math.abs(Math.sin(i * 12.9898 + 42) * 43758.5453) % 1

        let keep = false
        if (maskData) {
          const lat = (Math.asin(Math.max(-1, Math.min(1, y))) * 180) / Math.PI
          const lon = (Math.atan2(x, z) * 180) / Math.PI
          const u = (((lon + 180) / 360) % 1 + 1) % 1
          const v = Math.max(0, Math.min(0.9999, (90 - lat) / 180))
          const px = Math.floor(u * mWidth)
          const py = Math.floor(v * mHeight)
          const idx = (py * mWidth + px) * 4
          const coverage = maskData[idx] / 255.0

          if (coverage > 0.06 && hash < coverage) {
            keep = true
          }
        } else {
          keep = hash < 0.24
        }

        if (keep) {
          newPoints.push({ x, y, z })
        }
      }

      points = newPoints
    }

    const img = new Image()
    img.src = landMaskUrl
    img.onload = () => initPoints(img)
    img.onerror = () => initPoints()
    if (img.complete) {
      initPoints(img)
    } else {
      initPoints()
    }

    const updateSize = (): void => {
      const rect = canvas.getBoundingClientRect()
      const dpr = window.devicePixelRatio || 1
      canvas.width = Math.floor(rect.width * dpr)
      canvas.height = Math.floor(rect.height * dpr)
    }
    updateSize()
    const resizeObserver = new ResizeObserver(updateSize)
    resizeObserver.observe(canvas)

    const handleMouseDown = (e: MouseEvent): void => {
      isDragging = true
      focusTargetRef.current = null
      lastMouseX = e.clientX
      lastMouseY = e.clientY
    }

    const handleMouseMove = (e: MouseEvent): void => {
      if (!isDragging) return
      const dx = e.clientX - lastMouseX
      const dy = e.clientY - lastMouseY
      lastMouseX = e.clientX
      lastMouseY = e.clientY

      targetPhi += dx * 0.008
      targetTheta = Math.max(-0.95, Math.min(0.95, targetTheta - dy * 0.008))
    }

    const handleMouseUp = (): void => {
      isDragging = false
    }

    canvas.addEventListener('mousedown', handleMouseDown)
    window.addEventListener('mousemove', handleMouseMove)
    window.addEventListener('mouseup', handleMouseUp)

    let pulseTime = 0

    const render = (): void => {
      pulseTime += 0.03

      if (focusTargetRef.current && !isDragging) {
        const twoPi = Math.PI * 2
        let dPhi = (focusTargetRef.current.phi - targetPhi) % twoPi
        if (dPhi > Math.PI) dPhi -= twoPi
        if (dPhi < -Math.PI) dPhi += twoPi
        targetPhi += dPhi * 0.08
        targetTheta += (focusTargetRef.current.theta - targetTheta) * 0.08
        if (Math.abs(dPhi) < 0.01 && Math.abs(focusTargetRef.current.theta - targetTheta) < 0.01) {
          focusTargetRef.current = null
        }
      } else if (!isDragging) {
        targetPhi += activeRef.current ? 0.0022 : 0.0012
      }

      phi += (targetPhi - phi) * 0.1
      theta += (targetTheta - theta) * 0.1

      if (rotationRef?.current) {
        rotationRef.current.phi = phi
        rotationRef.current.theta = theta
      }

      const width = canvas.width
      const height = canvas.height
      const dpr = window.devicePixelRatio || 1
      ctx.clearRect(0, 0, width, height)

      if (width === 0 || height === 0) {
        animationFrameId = requestAnimationFrame(render)
        return
      }

      const [dr, dg, db] = hexToRgb(dotColorRef.current)
      const [ar, ag, ab] = hexToRgb(accentColorRef.current)
      const isConnected = activeRef.current

      const centerX = width / 2
      const centerY = height / 2
      const radius = Math.min(width, height) * 0.44

      const cosPhi = Math.cos(phi)
      const sinPhi = Math.sin(phi)
      const cosTheta = Math.cos(theta)
      const sinTheta = Math.sin(theta)

      // Directional spherical lighting (-0.4, 0.55, 0.85)
      const lx = -0.4
      const ly = 0.55
      const lz = 0.85
      const lightLen = Math.sqrt(lx * lx + ly * ly + lz * lz)
      const nlx = lx / lightLen
      const nly = ly / lightLen
      const nlz = lz / lightLen

      // 1. Subtle soft monochrome edge halo
      const haloGrad = ctx.createRadialGradient(
        centerX,
        centerY,
        radius * 0.86,
        centerX,
        centerY,
        radius * 1.14
      )
      haloGrad.addColorStop(0, `rgba(${dr}, ${dg}, ${db}, 0)`)
      haloGrad.addColorStop(0.75, `rgba(${dr}, ${dg}, ${db}, ${isConnected ? 0.08 : 0.04})`)
      haloGrad.addColorStop(1, 'rgba(0, 0, 0, 0)')

      ctx.save()
      ctx.fillStyle = haloGrad
      ctx.beginPath()
      ctx.arc(centerX, centerY, radius * 1.14, 0, Math.PI * 2)
      ctx.fill()
      ctx.restore()

      // 2. Sphere volume shading (subtle top-left highlight + crisp 1px rim)
      const bodyGrad = ctx.createRadialGradient(
        centerX - radius * 0.32,
        centerY - radius * 0.32,
        radius * 0.05,
        centerX,
        centerY,
        radius * 1.25
      )
      bodyGrad.addColorStop(0, `rgba(${dr}, ${dg}, ${db}, 0.11)`)
      bodyGrad.addColorStop(0.55, `rgba(${dr}, ${dg}, ${db}, 0.03)`)
      bodyGrad.addColorStop(1, 'rgba(0, 0, 0, 0)')

      ctx.save()
      ctx.fillStyle = bodyGrad
      ctx.beginPath()
      ctx.arc(centerX, centerY, radius, 0, Math.PI * 2)
      ctx.fill()

      ctx.strokeStyle = `rgba(${dr}, ${dg}, ${db}, ${isConnected ? 0.22 : 0.16})`
      ctx.lineWidth = Math.max(1, dpr * 0.9)
      ctx.stroke()
      ctx.restore()

      // 3. Monochrome Dotted Continents (Lambertian alpha formula)
      const pointScale = Math.max(0.7, (radius / 260) * 1.0)
      ctx.save()
      for (let i = 0; i < points.length; i++) {
        const p = points[i]
        const x1 = p.x * cosPhi + p.z * sinPhi
        const z1 = -p.x * sinPhi + p.z * cosPhi
        const y2 = p.y * cosTheta - z1 * sinTheta
        const z2 = p.y * sinTheta + z1 * cosTheta

        if (z2 < -0.05) continue

        const depth = (z2 + 1) / 2
        const brightness = (x1 * nlx + y2 * nly + z2 * nlz + 1) / 2
        const baseAlpha = (0.22 + 0.78 * brightness) * (0.35 + 0.65 * depth)
        const alpha = Math.min(0.92, Math.max(0.08, isConnected ? baseAlpha * 0.85 : baseAlpha * 0.55))

        const sx = centerX + x1 * radius
        const sy = centerY - y2 * radius
        const dotRadius = (0.9 + 0.65 * depth) * pointScale

        ctx.fillStyle = `rgba(${dr}, ${dg}, ${db}, ${alpha.toFixed(2)})`
        ctx.beginPath()
        ctx.arc(sx, sy, dotRadius, 0, Math.PI * 2)
        ctx.fill()
      }
      ctx.restore()

      // 4. Server Marker Beacon + DOM Popover Tracking
      const marker = markerRef.current
      const pill = pillRef.current
      if (marker) {
        const latRad = (marker.lat * Math.PI) / 180
        const lonRad = (marker.lon * Math.PI) / 180

        const mx0 = Math.cos(latRad) * Math.sin(lonRad)
        const my0 = Math.sin(latRad)
        const mz0 = Math.cos(latRad) * Math.cos(lonRad)

        const mx1 = mx0 * cosPhi + mz0 * sinPhi
        const mz1 = -mx0 * sinPhi + mz0 * cosPhi
        const my2 = my0 * cosTheta - mz1 * sinTheta
        const mz2 = my0 * sinTheta + mz1 * cosTheta

        const visible = mz2 > -0.05
        const msx = centerX + mx1 * radius
        const msy = centerY - my2 * radius
        const mDepth = Math.max(0.25, (mz2 + 1) / 2)

        if (visible) {
          ctx.save()
          const pulseRadius = (7 + Math.sin(pulseTime * 2.5) * 2.5) * dpr
          ctx.strokeStyle = `rgba(${ar}, ${ag}, ${ab}, ${(0.65 * mDepth).toFixed(2)})`
          ctx.lineWidth = 1.4 * dpr
          ctx.beginPath()
          ctx.arc(msx, msy, pulseRadius, 0, Math.PI * 2)
          ctx.stroke()

          ctx.fillStyle = accentColorRef.current
          ctx.shadowColor = accentColorRef.current
          ctx.shadowBlur = 12 * dpr
          ctx.beginPath()
          ctx.arc(msx, msy, 4 * dpr, 0, Math.PI * 2)
          ctx.fill()
          ctx.shadowBlur = 0
          ctx.restore()
        }

        if (pill) {
          const cssX = msx / dpr
          const cssY = msy / dpr
          pill.style.transform = `translate3d(${cssX.toFixed(1)}px, ${cssY.toFixed(1)}px, 0) translate(-50%, -140%)`
          pill.style.opacity = visible ? (0.45 + 0.55 * mDepth).toFixed(2) : '0'
        }
      } else if (pill) {
        pill.style.opacity = '0'
      }

      animationFrameId = requestAnimationFrame(render)
    }

    render()

    return () => {
      cancelAnimationFrame(animationFrameId)
      resizeObserver.disconnect()
      canvas.removeEventListener('mousedown', handleMouseDown)
      window.removeEventListener('mousemove', handleMouseMove)
      window.removeEventListener('mouseup', handleMouseUp)
    }
  }, [rotationRef])

  const parsedMarker = activeServerName ? extractLeadingFlagAndTitle(activeServerName) : null

  return (
    <div
      className={`relative w-full h-full select-none cursor-grab active:cursor-grabbing ${className}`}
    >
      <canvas ref={canvasRef} className="w-full h-full block" />
      {parsedMarker && (
        <div
          ref={pillRef}
          style={{ opacity: 0 }}
          className="pointer-events-none absolute top-0 left-0 flex items-center gap-1.5 rounded-lg bg-[#171717]/95 border border-white/15 px-2.5 py-1 shadow-lg transition-opacity duration-150 whitespace-nowrap"
        >
          {parsedMarker.flag && (
            <span className="flag-emoji text-xs leading-none">{parsedMarker.flag}</span>
          )}
          <span className="text-[11px] font-medium text-[#FAFAFA] max-w-44 truncate">
            {parsedMarker.title}
          </span>
        </div>
      )}
    </div>
  )
}

export default NullexGlobe
