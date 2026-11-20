import React from 'react';
import { Link } from 'react-router-dom';
import { Spinner, Tag, TextLink } from '@jamiehales/ui-kit/mostlymaking';
import { useModelProcessing } from '../lib/queries';
import type { ProcessingEntry } from '../lib/api';
import styles from './ModelProcessingPanel.module.css';

const SPLIT_METHODS: Record<string, string> = {
  none: 'None',
  shells: 'Separate shells',
  welded: 'Welded',
  twin: 'Twin',
  twin_remeshed: 'Remeshed twin',
};

const LOOKUP_VERDICTS: Record<string, string> = {
  none: 'None',
  unverified: 'Unverified',
  refused: 'Refused',
  twin: 'Twin',
  twin_remeshed: 'Remeshed twin',
};

const WHEN = new Intl.DateTimeFormat('en-GB', {
  day: 'numeric',
  month: 'short',
  year: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
});

const COUNT = new Intl.NumberFormat('en-GB');

function label(names: Record<string, string>, value: string): string {
  return names[value] ?? value.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());
}

/** A checksum shortened to its ends: `9c41e7a2…f03b`. */
function shortChecksum(checksum: string): string {
  return checksum.length > 16 ? `${checksum.slice(0, 8)}…${checksum.slice(-4)}` : checksum;
}

function num(detail: ProcessingEntry['detail'], key: string): number | null {
  const value = detail?.[key];
  return typeof value === 'number' ? value : null;
}

/** A recorded version, with the current one when it has moved on. */
function version(recorded: number | null | undefined, current: number): string | null {
  if (recorded == null) return null;
  return recorded === current ? `v${recorded}` : `v${recorded}, now v${current}`;
}

function Group({ title, at, children }: { title: string; at?: string; children: React.ReactNode }) {
  return (
    <section className={styles.group} aria-label={title}>
      <div className={styles.groupHead}>
        <h3 className="mm-type-label">{title}</h3>
        {at && (
          <time className={styles.time} dateTime={at}>
            {WHEN.format(new Date(at))}
          </time>
        )}
      </div>
      <dl className={`mm-type-small ${styles.rows}`}>{children}</dl>
    </section>
  );
}

function Row({
  label,
  mono,
  children,
}: {
  label: string;
  mono?: boolean;
  children: React.ReactNode;
}) {
  return (
    <>
      <dt>{label}</dt>
      <dd className={mono ? styles.mono : undefined}>{children}</dd>
    </>
  );
}

/** How a model was processed, for tracing a problem with its split or repair back. */
function ModelProcessingPanel({ modelId }: { modelId: string }) {
  const { data, isPending, isError } = useModelProcessing(modelId);

  if (isPending) {
    return (
      <div className={styles.panel}>
        <div className={styles.status}>
          <Spinner label="Loading processing data" />
        </div>
      </div>
    );
  }

  if (isError) {
    return (
      <div className={styles.panel}>
        <p className={`mm-type-small ${styles.status}`}>Processing data could not be loaded.</p>
      </div>
    );
  }

  const of = (kind: string) => data.processing.find((p) => p.kind === kind);
  const split = of('support_split');
  const lookup = of('twin_lookup');
  const repair = of('repair');
  const current = data.currentVersions;

  if (!split && !lookup && !repair && data.twins.length === 0) {
    return (
      <div className={styles.panel}>
        <p className={`mm-type-small ${styles.status}`}>No processing data yet.</p>
      </div>
    );
  }

  const candidates = Array.isArray(lookup?.detail?.verified) ? lookup.detail.verified.length : null;
  const remeshed = split?.method === 'twin_remeshed' || lookup?.method === 'twin_remeshed';
  const versions = [
    { label: 'Detection', value: version(split?.detectionVersion, current.detection) },
    { label: 'Support reader', value: version(split?.readerVersion, current.reader) },
    {
      label: 'Twin',
      value: version(split?.twinVersion ?? lookup?.twinVersion, current.twin),
    },
    {
      label: 'Remesh twin',
      value: remeshed
        ? version(num(lookup?.detail ?? null, 'remeshVersion'), current.remeshTwin)
        : null,
    },
    { label: 'Repair', value: version(repair?.repairVersion, current.repair) },
  ].filter((v): v is { label: string; value: string } => v.value != null);

  const r = repair?.detail ?? null;
  const repairMs = num(r, 'repairMs') ?? repair?.durationMs ?? null;
  const count = (key: string) => {
    const n = num(r, key);
    return n == null ? null : COUNT.format(n);
  };

  return (
    <div className={styles.panel}>
      {split && (
        <Group title="Supports" at={split.processedAt}>
          <Row label="Split method">{label(SPLIT_METHODS, split.method)}</Row>
          <Row label="Source checksum" mono>
            <span title={split.sourceChecksum}>{shortChecksum(split.sourceChecksum)}</span>
          </Row>
        </Group>
      )}

      {data.twins.length > 0 && (
        <Group title="Twin" at={split?.processedAt}>
          {data.twins.map((twin) => (
            <React.Fragment key={twin.twinId}>
              <Row label="Model">
                <TextLink asChild className={styles.link}>
                  <Link to={`/model/${encodeURIComponent(twin.twinId)}`} title={twin.relativePath}>
                    {twin.name}
                  </Link>
                </TextLink>
              </Row>
              <Row label="Scale" mono>
                {twin.scale.toFixed(3)}
              </Row>
              <Row label="Coverage" mono>
                {(twin.coverage * 100).toFixed(1)}%
              </Row>
              <Row label="Status">
                <Tag variant={twin.current ? 'success' : 'default'} className={styles.tag}>
                  {twin.current ? 'Current' : 'Out of date'}
                </Tag>
              </Row>
            </React.Fragment>
          ))}
        </Group>
      )}

      {lookup && (
        <Group title="Twin lookup" at={lookup.processedAt}>
          <Row label="Verdict">{label(LOOKUP_VERDICTS, lookup.method)}</Row>
          {candidates != null && (
            <Row label="Candidates" mono>
              {COUNT.format(candidates)} checked
            </Row>
          )}
        </Group>
      )}

      {repair && (
        <Group title="Repair" at={repair.processedAt}>
          {count('localMended') != null && (
            <Row label="Local mends" mono>
              {count('localMended')} mended, {count('localLeft') ?? 0} left
            </Row>
          )}
          {count('mendBoxes') != null && (
            <Row label="Box mends" mono>
              {count('mendBoxes')}
            </Row>
          )}
          {count('followUpPasses') != null && (
            <Row label="Follow-up passes" mono>
              {count('followUpPasses')}
            </Row>
          )}
          {count('facesIn') != null && (
            <Row label="Faces" mono>
              {count('facesIn')} in, {count('facesOut') ?? 0} out
            </Row>
          )}
          {repairMs != null && (
            <Row label="Repair time" mono>
              {(repairMs / 1000).toFixed(1)} s
            </Row>
          )}
        </Group>
      )}

      {versions.length > 0 && (
        <Group title="Versions">
          {versions.map((v) => (
            <Row key={v.label} label={v.label} mono>
              {v.value}
            </Row>
          ))}
        </Group>
      )}
    </div>
  );
}

export default ModelProcessingPanel;
