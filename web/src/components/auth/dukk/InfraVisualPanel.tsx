// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/**
 * Painel visual (coluna esquerda) das telas de autenticação do Portal da Infra.
 *
 * Porte React do `LoginVisualPanel.vue` do CRM (`dukk-front-web`), mantendo o
 * mesmo desenho: fundo pontilhado à deriva com máscara radial, parallax sutil
 * do grupo conforme o mouse, traçado tracejado e quatro nós com tooltip no
 * hover. O que muda é a cópia e os quatro nós — aqui eles nomeiam as
 * capacidades deste portal, não os recursos do workspace.
 *
 * Os ícones vêm do `lucide-react` (o CRM usa Phosphor, que não é dependência
 * do Temps): trazer outra biblioteca de ícone só para quatro glifos custaria
 * mais ao bundle do que qualquer ganho de fidelidade.
 *
 * Sem estado de auth. O tema só troca a cor de fundo do painel; o resto herda
 * os tokens `--c-*` do wrapper `.dukk-ds`.
 */

import { Cpu, Database, HardDrive, ShieldCheck } from 'lucide-react'
import type { ComponentType, SVGProps } from 'react'
import { useCallback, useRef, useState } from 'react'

interface OrbitNode {
  id: string
  left: string
  top: string
  scale: number
  label: string
  Icon: ComponentType<SVGProps<SVGSVGElement>>
}

/**
 * Os rótulos nomeiam o que o portal FAZ, não como a infraestrutura por trás
 * dele está montada. Este repositório é público e uma tela de login é lida por
 * qualquer um que alcance a porta: a topologia, o provedor de identidade e os
 * arranjos de isolamento e retenção não são informação que ajude quem está
 * entrando, e são exatamente o tipo de detalhe que não se recolhe depois.
 */
const NODES: OrbitNode[] = [
  {
    id: 'acesso',
    left: '4%',
    top: '8%',
    scale: 0.85,
    label: 'Acesso único corporativo',
    Icon: ShieldCheck,
  },
  {
    id: 'implantacoes',
    left: '68%',
    top: '7%',
    scale: 1.1,
    label: 'Builds e implantações',
    Icon: Cpu,
  },
  {
    id: 'bancos',
    left: '84%',
    top: '62%',
    scale: 0.92,
    label: 'Bancos gerenciados',
    Icon: Database,
  },
  {
    id: 'backups',
    left: '30%',
    top: '75%',
    scale: 1.2,
    label: 'Backups e restauração',
    Icon: HardDrive,
  },
]

export function InfraVisualPanel({
  theme = 'dark',
}: {
  /** Só afeta a cor de fundo do painel. */
  theme?: 'dark' | 'light'
}) {
  const panelBg = theme === 'light' ? '#2B1B17' : '#131215'

  const spaceRef = useRef<HTMLDivElement | null>(null)
  const [drift, setDrift] = useState({ dx: 0, dy: 0 })
  const [hovered, setHovered] = useState<string | null>(null)

  const onMouseMove = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const el = spaceRef.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    setDrift({
      dx: (e.clientX - rect.left) / rect.width - 0.5,
      dy: (e.clientY - rect.top) / rect.height - 0.5,
    })
  }, [])

  const onMouseLeave = useCallback(() => setDrift({ dx: 0, dy: 0 }), [])

  return (
    <div
      className="dukk-visual-panel relative flex min-h-screen flex-col overflow-hidden"
      style={{ background: panelBg, color: 'rgb(var(--c-dukk-cream))' }}
    >
      {/* Fundo pontilhado à deriva. */}
      <div
        className="dukk-bg-drift pointer-events-none absolute"
        style={{
          inset: '-40px',
          backgroundImage:
            'radial-gradient(rgb(255 255 255 / 0.16) 1.2px, transparent 1.3px)',
          backgroundSize: '18px 18px',
          WebkitMaskImage:
            'radial-gradient(ellipse 78% 78% at 38% 55%, transparent 0%, #000 42%, transparent 100%)',
          maskImage:
            'radial-gradient(ellipse 78% 78% at 38% 55%, transparent 0%, #000 42%, transparent 100%)',
        }}
      />

      <div
        className="relative z-[2] flex h-full flex-1 flex-col"
        style={{ padding: '44px 72px' }}
      >
        <div className="flex h-9 items-center gap-2.5">
          <span
            style={{
              fontSize: '12.5px',
              fontWeight: 600,
              color: 'rgb(255 255 255 / 0.6)',
              letterSpacing: '0.04em',
              textTransform: 'uppercase',
            }}
          >
            Portal da Infra
          </span>
        </div>

        <div
          className="flex flex-1 flex-col items-start justify-center text-left"
          style={{ maxWidth: '480px', margin: '0 auto' }}
        >
          <img
            src="/brand/dukk-mark-white.svg"
            alt="Dukk"
            style={{
              width: '60px',
              height: '60px',
              objectFit: 'contain',
              marginBottom: '28px',
            }}
          />

          <h1
            style={{
              fontFamily: 'var(--font-display)',
              fontWeight: 600,
              fontSize: 'clamp(30px,3vw,40px)',
              lineHeight: 1.16,
              letterSpacing: '-0.015em',
              margin: '0 0 16px',
              color: '#fff',
              textWrap: 'pretty',
            }}
          >
            Portal da Infra: o plano de controle das máquinas que rodam o Dukk.
          </h1>
          <p
            style={{
              fontSize: '16px',
              lineHeight: 1.65,
              color: 'rgb(255 255 255 / 0.72)',
              margin: '0 0 40px',
              maxWidth: '42ch',
              textWrap: 'pretty',
            }}
          >
            Entre com sua conta corporativa para provisionar ambientes, acompanhar
            builds e operar bancos e backups — tudo em infraestrutura própria.
          </p>

          {/* Espaço dos nós orbitando. */}
          <div
            ref={spaceRef}
            className="relative w-full"
            style={{ maxWidth: '520px', height: '290px', marginTop: '32px' }}
            onMouseMove={onMouseMove}
            onMouseLeave={onMouseLeave}
          >
            <div
              style={{
                position: 'absolute',
                inset: 0,
                transform: `translate(${drift.dx * 14}px, ${drift.dy * 14}px)`,
                transition: 'transform 0.2s ease-out',
              }}
            >
              <svg
                width="100%"
                height="290"
                viewBox="0 0 520 290"
                aria-hidden="true"
                style={{
                  position: 'absolute',
                  inset: 0,
                  overflow: 'visible',
                  pointerEvents: 'none',
                }}
              >
                <path
                  d="M21,24 C108,-24 240,84 360,19 C480,-36 516,132 446,175 C384,228 216,108 158,214"
                  fill="none"
                  stroke="rgb(255 255 255 / 0.22)"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                  strokeDasharray="1.5 9"
                />
              </svg>

              {NODES.map((node) => {
                const isHovered = hovered === node.id
                return (
                  <div
                    key={node.id}
                    style={{
                      position: 'absolute',
                      left: node.left,
                      top: node.top,
                      transform: `translate(-50%,-100%) scale(${node.scale})`,
                      transformOrigin: 'center bottom',
                      cursor: 'pointer',
                    }}
                    onMouseEnter={() => setHovered(node.id)}
                    onMouseLeave={() =>
                      setHovered((current) =>
                        current === node.id ? null : current
                      )
                    }
                  >
                    <div
                      style={{
                        width: '52px',
                        height: '52px',
                        borderRadius: '14px 14px 14px 4px',
                        background: '#2c2a30',
                        display: 'grid',
                        placeItems: 'center',
                        boxShadow: '0 12px 22px rgb(0 0 0 / 0.4)',
                        transition: 'background 0.2s, transform 0.15s',
                        transform: isHovered ? 'scale(1.1)' : 'scale(1)',
                      }}
                    >
                      <node.Icon
                        width={25}
                        height={25}
                        color="rgb(var(--c-lime))"
                        aria-hidden="true"
                      />
                    </div>
                    {/* "rabinho" do balão */}
                    <div
                      style={{
                        width: '15px',
                        height: '15px',
                        background: '#2c2a30',
                        transform: 'rotate(45deg)',
                        margin: '-7.5px auto 0',
                        borderRadius: '0 0 4px 0',
                      }}
                    />
                    <div
                      style={{
                        position: 'absolute',
                        top: '26px',
                        left: '26px',
                        transform: `translate(-50%,-50%) scale(${isHovered ? 1 : 0.92})`,
                        whiteSpace: 'nowrap',
                        background: 'rgb(20 19 22 / 0.96)',
                        borderRadius: '8px',
                        padding: '7px 12px',
                        fontSize: '12.5px',
                        fontWeight: 600,
                        color: '#fff',
                        opacity: isHovered ? 1 : 0,
                        pointerEvents: 'none',
                        transition: 'opacity 0.18s ease, transform 0.18s ease',
                        zIndex: 5,
                        boxShadow: '0 8px 20px rgb(0 0 0 / 0.4)',
                      }}
                    >
                      {node.label}
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
