import * as React from 'react';
import Button from '@mui/material/Button';
import Dialog from '@mui/material/Dialog';
import DialogActions from '@mui/material/DialogActions';
import DialogContent from '@mui/material/DialogContent';
import DialogTitle from '@mui/material/DialogTitle';

export default function ConfirmationDialog(props) {

  return (
      <Dialog
          open={props.open}
          onClose={props.onClose}
          aria-labelledby="confirmation-dialog-title"
          aria-describedby="confirmation-dialog-description"
      >
        <DialogTitle id="confirmation-dialog-title">
          {props.title}
        </DialogTitle>
        <DialogContent>
          {props.children}
        </DialogContent>
        <DialogActions>
          <Button onClick={props.onClose} autoFocus>
            Close
          </Button>
        </DialogActions>
      </Dialog>
  );
}