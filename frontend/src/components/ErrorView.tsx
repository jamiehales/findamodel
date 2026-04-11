import Typography from '@mui/material/Typography';
import Stack from '@mui/material/Stack';

interface ErrorViewProps {
  message?: string;
  minHeight?: string | number;
}

export default function ErrorView({
  message = 'Something went wrong.',
  minHeight = '30vh',
}: ErrorViewProps) {
  return (
    <Stack alignItems="center" justifyContent="center" sx={{ minHeight, gap: 2 }}>
      <Typography color="error.main">{message}</Typography>
    </Stack>
  );
}
