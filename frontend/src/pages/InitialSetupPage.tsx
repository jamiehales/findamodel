import { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  FieldError,
  Input,
  Label,
  Progress,
  Spinner,
  Switch,
} from '@jamiehales/ui-kit/mostlymaking';
import {
  useCompleteInitialSetup,
  useEnqueueIndex,
  useIndexerStatus,
  useInitialSetupDefaults,
} from '../lib/queries';
import { IndexFlags } from '../lib/api';
import styles from './InitialSetupPage.module.css';

interface InitialSetupPageProps {
  onCompleted: () => void;
}

type Step = 'library' | 'previews' | 'review' | 'done';

const STEPS: { key: Exclude<Step, 'done'>; label: string; title: string }[] = [
  { key: 'library', label: 'Library', title: 'Model library' },
  { key: 'previews', label: 'Previews', title: 'Previews and rafts' },
  { key: 'review', label: 'Review', title: 'Review' },
];

const numberFormat = new Intl.NumberFormat();

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <span className={`mm-type-logo ${styles.logo}`}>
          mostlymaking<span className={styles.dot}>.</span>
          <span className={styles.product}>findamodel</span>
        </span>
      </header>
      <main className={styles.main}>{children}</main>
    </div>
  );
}

/** Shown after setup: the first index run, with a way into the app. */
function IndexingStarted({ onFinish }: { onFinish: () => void }) {
  const { data: status } = useIndexerStatus({ adaptivePolling: true });
  const current = status?.currentRequest ?? null;
  const total = current?.totalFiles ?? null;
  const processed = current?.processedFiles ?? 0;

  return (
    <div className={styles.card}>
      <div className={styles.body}>
        <h1 className={styles.title}>Indexing your library</h1>
        <p className={styles.muted}>
          Indexing large libraries may take a while. Indexing will continue in the background.
        </p>
        <div className={styles.progress} role="status">
          <Progress
            value={total ? processed : undefined}
            max={total ?? 100}
            aria-label="Indexing progress"
          />
          {total != null && (
            <span className={styles.mono}>
              {numberFormat.format(processed)} / {numberFormat.format(total)}
            </span>
          )}
        </div>
      </div>
      <div className={styles.footer}>
        <div className={styles.spacer} />
        <Button variant="primary" onClick={onFinish}>
          Finish
        </Button>
      </div>
    </div>
  );
}

function InitialSetupPage({ onCompleted }: InitialSetupPageProps) {
  const defaultsQuery = useInitialSetupDefaults(true);
  const completeSetupMutation = useCompleteInitialSetup();
  const enqueueIndex = useEnqueueIndex();

  const [step, setStep] = useState<Step>('library');
  const [modelsDirectoryPath, setModelsDirectoryPath] = useState('');
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('2');
  const [generatePreviewsEnabled, setGeneratePreviewsEnabled] = useState(true);
  const [pathTouched, setPathTouched] = useState(false);
  const [errorText, setErrorText] = useState<string | null>(null);

  const raftHeight = Number(defaultRaftHeightMm);
  const raftValid =
    defaultRaftHeightMm.trim() !== '' && Number.isFinite(raftHeight) && raftHeight >= 0;
  const pathMissing = modelsDirectoryPath.trim().length === 0;

  useEffect(() => {
    if (!defaultsQuery.data) return;
    const defaults = defaultsQuery.data;
    setModelsDirectoryPath(defaults.modelsDirectoryPath ?? '');
    setDefaultRaftHeightMm(String(defaults.defaultRaftHeightMm));
    setGeneratePreviewsEnabled(defaults.generatePreviewsEnabled);
  }, [defaultsQuery.data]);

  if (defaultsQuery.isLoading) {
    return (
      <Shell>
        <div className={styles.card}>
          <div className={styles.center} role="status">
            <Spinner className={styles.bigSpinner} />
            Loading
          </div>
        </div>
      </Shell>
    );
  }

  if (defaultsQuery.isError) {
    return (
      <Shell>
        <div className={styles.card}>
          <div className={styles.body}>
            <h1 className={styles.title}>Setup</h1>
            <Alert tone="error">Couldn't reach the server. Check it's running, then retry.</Alert>
          </div>
          <div className={styles.footer}>
            <div className={styles.spacer} />
            <Button variant="secondary" onClick={() => defaultsQuery.refetch()}>
              Retry
            </Button>
          </div>
        </div>
      </Shell>
    );
  }

  if (step === 'done') {
    return (
      <Shell>
        <IndexingStarted onFinish={onCompleted} />
      </Shell>
    );
  }

  const stepIndex = STEPS.findIndex((s) => s.key === step);
  const current = STEPS[stepIndex];
  const saving = completeSetupMutation.isPending;

  const save = async () => {
    setErrorText(null);
    try {
      await completeSetupMutation.mutateAsync({
        modelsDirectoryPath: modelsDirectoryPath.trim(),
        defaultRaftHeightMm: raftHeight,
        generatePreviewsEnabled,
      });
      enqueueIndex.mutate({ directoryFilter: null, flags: IndexFlags.All });
      setStep('done');
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Couldn't save. Try again.");
    }
  };

  const handleSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (step === 'library') {
      setPathTouched(true);
      if (!pathMissing) setStep('previews');
    } else if (step === 'previews') {
      if (raftValid) setStep('review');
    } else if (!saving) {
      void save();
    }
  };

  const pathError = pathTouched && pathMissing;

  return (
    <Shell>
      <form className={styles.card} onSubmit={handleSubmit} noValidate>
        <div className={styles.body}>
          <nav className={styles.steps} aria-label="Setup steps">
            {STEPS.map((s, i) => (
              <button
                key={s.key}
                type="button"
                className={styles.step}
                data-state={i < stepIndex ? 'done' : i === stepIndex ? 'current' : 'todo'}
                aria-current={i === stepIndex ? 'step' : undefined}
                disabled={i >= stepIndex || saving}
                onClick={() => setStep(s.key)}
              >
                <i aria-hidden="true" />
                {s.label}
              </button>
            ))}
          </nav>

          <h1 className={styles.title}>{current.title}</h1>

          {step === 'library' && (
            <div className={styles.field}>
              <Label htmlFor="setup-root">Model root</Label>
              <Input
                id="setup-root"
                className={styles.mono}
                placeholder="/models"
                autoFocus
                value={modelsDirectoryPath}
                aria-invalid={pathError || undefined}
                aria-describedby={pathError ? 'setup-root-error' : undefined}
                onChange={(e) => setModelsDirectoryPath(e.target.value)}
              />
              {pathError && (
                <FieldError id="setup-root-error">
                  Enter the folder that holds your models.
                </FieldError>
              )}
            </div>
          )}

          {step === 'previews' && (
            <>
              <div className={styles.option}>
                <Switch
                  checked={generatePreviewsEnabled}
                  onCheckedChange={(checked) => setGeneratePreviewsEnabled(checked)}
                >
                  Generate previews while indexing
                </Switch>
              </div>
              <div className={styles.field}>
                <Label htmlFor="setup-raft">Default raft height</Label>
                <div className={styles.unit}>
                  <Input
                    id="setup-raft"
                    type="number"
                    min={0}
                    step="any"
                    value={defaultRaftHeightMm}
                    aria-invalid={!raftValid || undefined}
                    onChange={(e) => setDefaultRaftHeightMm(e.target.value)}
                  />
                  <span>mm</span>
                </div>
                {!raftValid && <FieldError>Enter a height of 0 mm or more.</FieldError>}
              </div>
            </>
          )}

          {step === 'review' && (
            <>
              <dl className={styles.review}>
                <dt>Model root</dt>
                <dd className={styles.mono}>{modelsDirectoryPath.trim()}</dd>
                <dd>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={saving}
                    onClick={() => setStep('library')}
                  >
                    Edit
                  </Button>
                </dd>
                <dt>Previews</dt>
                <dd>{generatePreviewsEnabled ? 'On' : 'Off'}</dd>
                <dd>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={saving}
                    onClick={() => setStep('previews')}
                  >
                    Edit
                  </Button>
                </dd>
                <dt>Default raft height</dt>
                <dd className={styles.mono}>{raftHeight} mm</dd>
                <dd>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={saving}
                    onClick={() => setStep('previews')}
                  >
                    Edit
                  </Button>
                </dd>
              </dl>
              {errorText && !saving && <Alert tone="error">{errorText}</Alert>}
            </>
          )}
        </div>

        <div className={styles.footer}>
          {stepIndex > 0 && (
            <Button
              variant="ghost"
              disabled={saving}
              onClick={() => setStep(STEPS[stepIndex - 1].key)}
            >
              Back
            </Button>
          )}
          <div className={styles.spacer} />
          <span className={styles.mono}>Step {stepIndex + 1} of 3</span>
          <Button variant="primary" type="submit" disabled={saving}>
            {saving ? <Spinner label="Saving" /> : null}
            {step === 'review' ? (saving ? 'Saving' : errorText ? 'Try again' : 'Finish') : 'Next'}
          </Button>
        </div>
      </form>
    </Shell>
  );
}

export default InitialSetupPage;
