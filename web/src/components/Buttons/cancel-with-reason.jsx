import React, { Component } from 'react';

import {Button, Dialog, DialogActions, DialogContent, TextField} from "@mui/material";

export class CancelWithReasonButton extends Component {

    constructor(props) {
        super(props);
        this.state = { open: false, note: undefined }
    }

    __open = () => {
        this.setState({ open: true, note: undefined })
    }

    __close = () => {
        this.setState({ open: false, note: undefined })
    }

    __textChange = (event) => {
        this.setState({
           note: event.target.value,
        });
    }

    __confirmed = () => {
        const { note } = this.state;
        if(note && note.length > 0) {
            this.setState({open: false})
            this.props.onConfirmed(note);
        }
    }

    render() {

        let { open, note } = this.state;

        return (
            <>
                <Button { ...this.props } onClick={this.__open}>{this.props.children}</Button>
                <Dialog open={open}>
                    <DialogContent>{ "Please add a note." }</DialogContent>
                    <DialogContent>
                        <TextField id="cancel-input" label="Note" variant="outlined" onChange={this.__textChange} value={note} />
                    </DialogContent>
                    <DialogActions>
                        <Button autoFocus onClick={this.__close} variant={"outlined"} color="default">
                            Cancel
                        </Button>
                        <Button disabled={!note} onClick={this.__confirmed} variant={"outlined"} color="primary">
                            Yes
                        </Button>
                    </DialogActions>
                </Dialog>
            </>
        );
    }
}
