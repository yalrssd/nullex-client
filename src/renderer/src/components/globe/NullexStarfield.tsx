import React, { useEffect, useRef } from 'react'

export interface GlobeRotationState {
  phi: number
  theta: number
}

interface Star {
  x: number
  y: number
  radius: number
  alpha: number
  phase: number
  speed: number
}

interface NullexStarfieldProps {
  count?: number
  rotationRef?: React.RefObject<GlobeRotationState>
  pixelsPerRadian?: number
  className?: string
}

export const NullexStarfield: React.FC<NullexStarfieldProps> = ({
  count = 95,
  rotationRef,
  pixelsPerRadian = 240,
  className = ''
}) => {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let animationId = 0
    const stars: Star[] = []

    // Deterministic low-contrast monochrome stars
    for (let i = 0; i < count; i++) {
      const s = Math.sin(i * 12.9898 + 7) * 43758.5453
      const r1 = s - Math.floor(s)
      const s2 = Math.sin((i + 37) * 78.233) * 43758.5453
      const r2 = s2 - Math.floor(s2)
      const s3 = Math.sin((i + 91) * 39.425) * 43758.5453
      const r3 = s3 - Math.floor(s3)
      stars.push({
        x: r1,
        y: r2,
        radius: 0.55 + r3 * 0.95,
        alpha: 0.1 + r3 * 0.24,
        phase: r1 * Math.PI * 2,
        speed: 0.015 + r2 * 0.018
      })
    }

    const updateSize = (): void => {
      const rect = canvas.getBoundingClientRect()
      const dpr = window.devicePixelRatio || 1
      canvas.width = Math.floor(rect.width * dpr)
      canvas.height = Math.floor(rect.height * dpr)
    }
    updateSize()
    const observer = new ResizeObserver(updateSize)
    observer.observe(canvas)

    let tick = 0
    const render = (): void => {
      tick += 1
      const w = canvas.width
      const h = canvas.height
      const dpr = window.devicePixelRatio || 1
      ctx.clearRect(0, 0, w, h)

      if (w > 0 && h > 0) {
        const rot = rotationRef?.current
        const offsetX = rot ? rot.phi * pixelsPerRadian * dpr : 0
        const offsetY = rot ? rot.theta * pixelsPerRadian * dpr : 0

        for (let i = 0; i < stars.length; i++) {
          const st = stars[i]
          const px = (((st.x * w + offsetX) % w) + w) % w
          const py = (((st.y * h + offsetY) % h) + h) % h
          const twinkle = st.alpha + Math.sin(tick * st.speed + st.phase) * 0.06
          ctx.fillStyle = `rgba(229, 229, 229, ${Math.max(0.04, twinkle).toFixed(3)})`
          ctx.beginPath()
          ctx.arc(px, py, st.radius * dpr, 0, Math.PI * 2)
          ctx.fill()
        }
      }

      animationId = requestAnimationFrame(render)
    }

    render()

    return () => {
      cancelAnimationFrame(animationId)
      observer.disconnect()
    }
  }, [count, pixelsPerRadian, rotationRef])

  return (
    <canvas
      ref={canvasRef}
      className={`pointer-events-none absolute inset-0 w-full h-full ${className}`}
    />
  )
}

export default NullexStarfield
