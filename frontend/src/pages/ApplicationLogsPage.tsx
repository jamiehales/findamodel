import { useMemo, useState } from 'react';
import {
  Alert,
  Button,
  MenuItem,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Typography,
} from '@mui/material';
import { Link } from 'react-router-dom';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
import { useApplicationLogs } from '../lib/queries';
import styles from './ApplicationLogsPage.module.css';

const LIMIT = 500;

export default function ApplicationLogsPage() {
  const [channel, setChannel] = useState('');
  const [severity, setSeverity] = useState('');
  const [showExceptionsOnly, setShowExceptionsOnly] = useState(false);
  const { data, isPending, isError, refetch, isRefetching } = useApplicationLogs(
    channel,
    severity,
    LIMIT,
  );

  const entries = useMemo(() => {
    if (!data) return [];
    if (!showExceptionsOnly) return data.entries;
    return data.entries.filter((entry) => !!entry.exception);
  }, [data, showExceptionsOnly]);

  if (isPending) return <LoadingView />;
  if (isError || !data) return <ErrorView message="Failed to load application logs." />;

  return (
    <PageLayout variant="medium" spacing={2}>
      <Stack direction="row" justifyContent="space-between" alignItems="center">
        <Typography component="h1" variant="page-title">
          Application Logs
        </Typography>
        <Button component={Link} to="/settings" variant="text">
          Back to Settings
        </Button>
      </Stack>

      <Stack direction="row" spacing={2} className={styles.filterRow}>
        <TextField
          select
          size="small"
          label="Channel"
          value={channel}
          onChange={(e) => setChannel(e.target.value)}
          className={styles.filterControl}
        >
          <MenuItem value="">All channels</MenuItem>
          {data.availableChannels.map((item) => (
            <MenuItem key={item} value={item}>
              {item}
            </MenuItem>
          ))}
        </TextField>

        <TextField
          select
          size="small"
          label="Minimum severity"
          value={severity}
          onChange={(e) => setSeverity(e.target.value)}
          className={styles.filterControl}
        >
          <MenuItem value="">All severities</MenuItem>
          {data.availableSeverities.map((item) => (
            <MenuItem key={item} value={item}>
              {item}
            </MenuItem>
          ))}
        </TextField>

        <Button
          variant={showExceptionsOnly ? 'contained' : 'outlined'}
          onClick={() => setShowExceptionsOnly((prev) => !prev)}
        >
          {showExceptionsOnly ? 'Showing exceptions only' : 'Filter to exceptions'}
        </Button>

        <Button variant="outlined" disabled={isRefetching} onClick={() => refetch()}>
          Refresh
        </Button>
      </Stack>

      <Alert severity="info">
        Showing newest {LIMIT} log entries from this running backend instance.
      </Alert>

      <TableContainer className={styles.tableContainer}>
        <Table size="small" stickyHeader>
          <TableHead>
            <TableRow>
              <TableCell className={styles.timestampCell}>Timestamp</TableCell>
              <TableCell className={styles.severityCell}>Severity</TableCell>
              <TableCell className={styles.channelCell}>Channel</TableCell>
              <TableCell>Message</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {entries.map((entry, index) => (
              <TableRow key={`${entry.timestamp}-${index}`}>
                <TableCell className={styles.timestampCell}>
                  {new Date(entry.timestamp).toLocaleString()}
                </TableCell>
                <TableCell className={styles.severityCell}>{entry.severity}</TableCell>
                <TableCell className={styles.channelCell}>{entry.channel}</TableCell>
                <TableCell>
                  <Stack spacing={1}>
                    <Typography variant="body2" className={styles.messageText}>
                      {entry.message}
                    </Typography>
                    {entry.exception && (
                      <Typography
                        variant="caption"
                        component="pre"
                        className={styles.exceptionText}
                      >
                        {entry.exception}
                      </Typography>
                    )}
                  </Stack>
                </TableCell>
              </TableRow>
            ))}
            {entries.length === 0 && (
              <TableRow>
                <TableCell colSpan={4}>
                  <Typography color="text.secondary">
                    No logs matched the selected filters.
                  </Typography>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>
    </PageLayout>
  );
}
